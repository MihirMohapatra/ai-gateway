use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use http::{Request, Response, StatusCode};
use tower::{Layer, Service};

fn extract_key<B>(req: &Request<B>) -> String {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.strip_prefix("Bearer ")
                .map(String::from)
                .unwrap_or_else(|| v.to_string())
        })
        .or_else(|| {
            req.headers()
                .get("X-API-Key")
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        })
        .or_else(|| {
            req.headers()
                .get("X-Forwarded-For")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Clone)]
pub struct RateLimitLayer {
    max_burst: NonZeroU32,
    period: Duration,
}

impl RateLimitLayer {
    pub fn new(max_requests: u32, period: Duration) -> Self {
        let max_burst = NonZeroU32::new(max_requests.max(1)).unwrap();
        Self { max_burst, period }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let quota = Quota::with_period(self.period)
            .unwrap()
            .allow_burst(self.max_burst);
        RateLimitService {
            inner,
            limiter: Arc::new(RateLimiter::keyed(quota)),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: Arc<DefaultKeyedRateLimiter<String>>,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RateLimitService<S>
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
        let key = extract_key(&req);

        if self.limiter.check_key(&key).is_ok() {
            let future = self.inner.call(req);
            Box::pin(future)
        } else {
            let res = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .body(ResBody::default())
                .unwrap();
            Box::pin(std::future::ready(Ok(res)))
        }
    }
}
