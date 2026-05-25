use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use std::sync::Arc;

#[derive(Clone)]
pub struct JwtAuth {
    pub secret: Arc<str>,
}

pub async fn auth_middleware(
    auth: axum::Extension<JwtAuth>,
    req: Request,
    next: Next,
) -> Result<Response, axum::response::Response> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) => {
            match decode::<serde_json::Value>(
                t,
                &DecodingKey::from_secret(auth.secret.as_bytes()),
                &Validation::new(Algorithm::HS256),
            ) {
                Ok(_) => Ok(next.run(req).await),
                Err(_) => Err(axum::http::StatusCode::UNAUTHORIZED.into_response()),
            }
        }
        None => Err(axum::http::StatusCode::UNAUTHORIZED.into_response()),
    }
}
