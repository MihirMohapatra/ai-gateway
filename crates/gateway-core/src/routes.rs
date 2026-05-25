use axum::{Router, routing::post};

pub fn router() -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/embeddings", post(embeddings))
}

async fn chat_completions() -> &'static str {
    "not implemented"
}

async fn embeddings() -> &'static str {
    "not implemented"
}
