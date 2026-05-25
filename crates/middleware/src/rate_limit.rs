use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};

pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
) -> Result<Response, axum::response::Response> {
    Ok(next.run(req).await)
}
