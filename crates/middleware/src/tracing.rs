use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use uuid::Uuid;

pub async fn tracing_middleware(
    mut req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();

    req.extensions_mut().insert(request_id.clone());

    let method = req.method().clone();
    let uri = req.uri().clone();

    let response = next.run(req).await;

    let duration = start.elapsed();
    let status = response.status();

    tracing::info!(
        method = %method,
        uri = %uri,
        status = %status,
        duration = ?duration,
        request_id = %request_id,
        "request completed"
    );

    response
}
