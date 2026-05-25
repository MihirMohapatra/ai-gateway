use std::sync::Arc;
use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

use adapters::anthropic::AnthropicClient;

use adapters::error::ProviderError;
use adapters::openai::OpenAIClient;
use adapters::request::{ChatCompletionRequest, ChatCompletionResponse};
use adapters::router::{ProviderMap, ProviderRouter};

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<dyn ProviderRouter>,
}

impl AppState {
    pub fn with_defaults(
        openai_key: impl Into<String>,
        openai_url: impl Into<String>,
        anthropic_key: impl Into<String>,
        anthropic_url: impl Into<String>,
    ) -> Arc<Self> {
        let openai = Arc::new(OpenAIClient::new(openai_key, openai_url));
        let anthropic = Arc::new(AnthropicClient::new(anthropic_key, anthropic_url));

        let router = ProviderMap::new()
            .with_provider("openai", openai)
            .with_provider("anthropic", anthropic)
            .with_default("openai");

        Arc::new(Self {
            router: Arc::new(router),
        })
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(handle_root))
        .route("/chat", post(handle_chat))
        .route("/chat/stream", post(handle_chat_stream))
        .with_state(state)
}

async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let provider = Some(req.model.as_str());

    match state.router.complete(&req, provider).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err(map_error(e)),
    }
}

pub async fn handle_chat_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<
    Sse<impl Stream<Item = Result<Event, Infallible>>>,
    (axum::http::StatusCode, Json<serde_json::Value>),
> {
    let provider = Some(req.model.as_str());

    let byte_stream = state.router.stream(&req, provider).await.map_err(map_error)?;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        use futures::StreamExt;
        let mut byte_stream = byte_stream;

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if bytes.as_ref() == b"\n" {
                        let _ = tx.send(Ok(Event::default().data(""))).await;
                    } else {
                        let text = String::from_utf8_lossy(&bytes);
                        let _ = tx.send(Ok(Event::default().data(text))).await;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

async fn handle_root() -> &'static str {
    "AI Gateway is running"
}

fn map_error(e: ProviderError) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    let (status, message) = match &e {
        ProviderError::RateLimited => (axum::http::StatusCode::TOO_MANY_REQUESTS, "Rate limited"),
        ProviderError::Timeout => (axum::http::StatusCode::GATEWAY_TIMEOUT, "Request timed out"),
        ProviderError::CircuitOpen => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "Circuit breaker open"),
        ProviderError::NoProvider => (axum::http::StatusCode::BAD_REQUEST, "No provider configured"),
        ProviderError::AllProvidersFailed => (axum::http::StatusCode::BAD_GATEWAY, "All providers failed"),
        ProviderError::Api { .. } => (axum::http::StatusCode::BAD_GATEWAY, "Provider API error"),
        _ => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };
    (
        status,
        Json(serde_json::json!({ "error": message })),
    )
}
