use std::sync::Arc;

use adapters::client::{CircuitBreakerClient, RetryClient};
use adapters::error::ProviderError;
use adapters::request::{ChatCompletionRequest, ChatCompletionResponse};
use adapters::router::{ModelNameRouter, ProviderId, ProviderMap};
use axum::{Json, Router, extract::State, routing::post};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub provider_map: Arc<ProviderMap>,
}

impl AppState {
    pub fn with_defaults(openai_key: String, openai_url: String, anthropic_key: String, anthropic_url: String) -> Self {
        let router = Box::new(ModelNameRouter);
        let map = ProviderMap::new(router);

        let openai = CircuitBreakerClient::new(
            RetryClient::new(
                adapters::openai::OpenAIClient::new(openai_key, openai_url),
                3,
                Duration::from_millis(200),
                Duration::from_secs(5),
            ),
            5,
            Duration::from_secs(30),
            1,
        );
        map.register(ProviderId::OpenAI, Arc::new(openai));

        let anthropic = CircuitBreakerClient::new(
            RetryClient::new(
                adapters::anthropic::AnthropicClient::new(anthropic_key, anthropic_url),
                3,
                Duration::from_millis(200),
                Duration::from_secs(5),
            ),
            5,
            Duration::from_secs(30),
            1,
        );
        map.register(ProviderId::Anthropic, Arc::new(anthropic));

        Self { provider_map: Arc::new(map) }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/embeddings", post(embeddings))
        .with_state(state)
}

async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (axum::http::StatusCode, Json<Value>)> {
    tracing::info!(model = %req.model, messages = %req.messages.len(), "chat completion request");
    match state.provider_map.chat_completion(req).await {
        Ok(resp) => Ok(Json(resp)),
        Err(ProviderError::AllProvidersFailed) => {
            Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "all providers failed"}))))
        }
        Err(e) => {
            Err((axum::http::StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))))
        }
    }
}

async fn embeddings(
    State(_state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Json<Value> {
    tracing::info!(model = %req.model, "embedding request");
    Json(serde_json::json!({ "status": "received", "model": req.model }))
}
