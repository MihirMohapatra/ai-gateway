use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Request, Response, StatusCode};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use tower::{Layer, Service};

#[derive(Clone)]
pub enum AuthMode {
    Jwt { secret: Arc<str> },
    ApiKey { valid_keys: Vec<String> },
}

#[derive(Clone)]
pub struct AuthLayer {
    mode: AuthMode,
}

impl AuthLayer {
    pub fn jwt(secret: impl Into<Arc<str>>) -> Self {
        Self { mode: AuthMode::Jwt { secret: secret.into() } }
    }

    pub fn api_keys(keys: Vec<String>) -> Self {
        Self { mode: AuthMode::ApiKey { valid_keys: keys } }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService { inner, mode: self.mode.clone() }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    mode: AuthMode,
}

fn extract_bearer_token<B>(req: &Request<B>) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from)
}

fn extract_api_key<B>(req: &Request<B>) -> Option<String> {
    req.headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for AuthService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
    S::Future: Send + 'static,
    ResBody: Default + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let valid = match &self.mode {
            AuthMode::Jwt { secret } => extract_bearer_token(&req).map_or(false, |token| {
                decode::<serde_json::Value>(
                    &token,
                    &DecodingKey::from_secret(secret.as_bytes()),
                    &Validation::new(Algorithm::HS256),
                )
                .is_ok()
            }),
            AuthMode::ApiKey { valid_keys } => extract_api_key(&req).map_or(false, |k| valid_keys.contains(&k)),
        };

        if valid {
            let future = self.inner.call(req);
            Box::pin(future)
        } else {
            let res = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(ResBody::default())
                .unwrap();
            Box::pin(std::future::ready(Ok(res)))
        }
    }
}
