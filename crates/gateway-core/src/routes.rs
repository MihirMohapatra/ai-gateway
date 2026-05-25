use std::sync::Arc;
use std::convert::Infallible;
use std::time::Instant;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

use adapters::anthropic::AnthropicClient;
use adapters::cache::{CacheBackend, CachingClient};
use adapters::client::{ByteStream, ProviderAdapter, RetryClient};
use adapters::error::ProviderError;
use adapters::guardrails::{GuardrailsClient, GuardrailsConfig};
use adapters::metering::{ConsoleMeter, MeterBackend, MeteringClient};
use adapters::openai::OpenAIClient;
use adapters::request::{ChatCompletionRequest, ChatCompletionResponse};
use adapters::response::GatewayResponse;
use adapters::router::{ProviderMap, ProviderRouter};

use cache::local::LocalCache;
use cache::redis::RedisCache;

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
        redis_url: Option<String>,
        _database_url: Option<String>,
    ) -> Arc<Self> {
        let openai_key = openai_key.into();
        let openai_url = openai_url.into();
        let anthropic_key = anthropic_key.into();
        let anthropic_url = anthropic_url.into();

        let cache_backend: Arc<dyn CacheBackend> = if let Some(url) = redis_url {
            match RedisCache::new(&url) {
                Ok(rc) => Arc::new(rc),
                Err(e) => {
                    tracing::warn!("redis connection failed, falling back to local cache: {e}");
                    Arc::new(LocalCache::new(std::time::Duration::from_secs(3600)))
                }
            }
        } else {
            Arc::new(LocalCache::new(std::time::Duration::from_secs(3600)))
        };

        let meter_backend: Arc<dyn MeterBackend> = Arc::new(ConsoleMeter);

        let make_pipeline = |client: Arc<dyn ProviderAdapter>, api_key: &str| -> Arc<dyn ProviderAdapter> {
            let client: Arc<dyn ProviderAdapter> = Arc::new(
                CircuitBreakerAdapter::new(client, 5, std::time::Duration::from_secs(30), 3),
            );
            let client: Arc<dyn ProviderAdapter> = Arc::new(
                RetryClient::new(client, 3, std::time::Duration::from_millis(500), std::time::Duration::from_secs(5)),
            );
            let client: Arc<dyn ProviderAdapter> = Arc::new(
                MeteringClient::new(client, Arc::clone(&meter_backend), api_key),
            );
            let client: Arc<dyn ProviderAdapter> = Arc::new(
                CachingClient::new(client, Arc::clone(&cache_backend), 3600),
            );
            let client: Arc<dyn ProviderAdapter> = Arc::new(
                GuardrailsClient::new(client, GuardrailsConfig::default()),
            );
            client
        };

        let openai = make_pipeline(
            Arc::new(OpenAIClient::new(&openai_key, &openai_url)),
            "default",
        );
        let anthropic = make_pipeline(
            Arc::new(AnthropicClient::new(&anthropic_key, &anthropic_url)),
            "default",
        );

        let router = ProviderMap::new()
            .with_provider("openai", openai)
            .with_provider("anthropic", anthropic)
            .with_default("openai");

        Arc::new(Self {
            router: Arc::new(router),
        })
    }
}

struct CircuitBreakerAdapter {
    inner: Arc<dyn ProviderAdapter>,
    state: std::sync::Mutex<CircuitState>,
    failure_threshold: u32,
    reset_timeout: std::time::Duration,
}

#[derive(Clone, PartialEq)]
enum CircuitState {
    Closed { failure_count: u32 },
    Open { until: Instant },
    HalfOpen,
}

impl CircuitBreakerAdapter {
    fn new(inner: Arc<dyn ProviderAdapter>, failure_threshold: u32, reset_timeout: std::time::Duration, _half_open_max: u32) -> Self {
        Self {
            inner,
            state: std::sync::Mutex::new(CircuitState::Closed { failure_count: 0 }),
            failure_threshold,
            reset_timeout,
        }
    }

    fn check_state(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        match state.clone() {
            CircuitState::Open { until } if Instant::now() < until => false,
            CircuitState::Open { .. } => {
                *state = CircuitState::HalfOpen;
                true
            }
            _ => true,
        }
    }

    fn record(&self, success: bool) {
        let mut state = self.state.lock().unwrap();
        match (&*state, success) {
            (CircuitState::HalfOpen, true) => *state = CircuitState::Closed { failure_count: 0 },
            (CircuitState::HalfOpen, false) => {
                *state = CircuitState::Open { until: Instant::now() + self.reset_timeout }
            }
            (CircuitState::Closed { failure_count }, false) => {
                let n = failure_count + 1;
                if n >= self.failure_threshold {
                    *state = CircuitState::Open { until: Instant::now() + self.reset_timeout };
                } else {
                    *state = CircuitState::Closed { failure_count: n };
                }
            }
            _ => {}
        }
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for CircuitBreakerAdapter {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        if !self.check_state() {
            return Err(ProviderError::CircuitOpen);
        }
        let result = self.inner.complete(req).await;
        self.record(result.is_ok());
        result
    }

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        if !self.check_state() {
            return Err(ProviderError::CircuitOpen);
        }
        let result = self.inner.stream(req).await;
        self.record(result.is_ok());
        result
    }

    fn token_count(&self, text: &str) -> usize {
        self.inner.token_count(text)
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
) -> Result<Json<GatewayResponse>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let provider_name = req.provider.as_deref().unwrap_or("openai");
    let start = Instant::now();

    match state.router.complete(&req, Some(provider_name)).await {
        Ok(response) => {
            let latency = start.elapsed().as_millis() as u64;
            Ok(Json(GatewayResponse::from_provider(response, provider_name, latency, false)))
        }
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
    let provider_name = req.provider.as_deref().unwrap_or("openai");

    let byte_stream = state.router.stream(&req, Some(provider_name)).await.map_err(map_error)?;

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
        ProviderError::CacheError(msg) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
        ProviderError::GuardrailsViolation(msg) => (axum::http::StatusCode::BAD_REQUEST, msg.as_str()),
        ProviderError::MeteringError(msg) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
        _ => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
    };
    (
        status,
        Json(serde_json::json!({ "error": message })),
    )
}
