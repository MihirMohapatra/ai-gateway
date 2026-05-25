use axum::{Json, Router, routing::post};
use serde_json::Value;

use crate::request::GatewayRequest;

pub fn router() -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/embeddings", post(embeddings))
}

async fn chat_completions(
    Json(req): Json<GatewayRequest>,
) -> Json<Value> {
    tracing::info!(model = %req.model, messages = %req.messages.len(), "chat completion request");
    Json(serde_json::json!({ "status": "received", "model": req.model }))
}

async fn embeddings(
    Json(req): Json<GatewayRequest>,
) -> Json<Value> {
    tracing::info!(model = %req.model, "embedding request");
    Json(serde_json::json!({ "status": "received", "model": req.model }))
}
