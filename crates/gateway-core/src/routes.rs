use std::sync::{Arc, RwLock};
use std::convert::Infallible;
use std::time::Instant;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use config::GatewayConfig;
use futures::stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tracing::instrument;

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

use crate::metrics::Metrics;

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<RwLock<Arc<dyn ProviderRouter>>>,
    pub metrics: Metrics,
}

fn build_router(config: &GatewayConfig) -> Arc<dyn ProviderRouter> {
    let cache_backend: Arc<dyn CacheBackend> = if let Some(ref url) = config.cache.redis_url {
        match RedisCache::new(url) {
            Ok(rc) => Arc::new(rc),
            Err(e) => {
                tracing::warn!("redis connection failed, falling back to local cache: {e}");
                Arc::new(LocalCache::new(std::time::Duration::from_secs(
                    config.cache.ttl_secs.unwrap_or(3600),
                )))
            }
        }
    } else {
        Arc::new(LocalCache::new(std::time::Duration::from_secs(
            config.cache.ttl_secs.unwrap_or(3600),
        )))
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
            CachingClient::new(client, Arc::clone(&cache_backend), config.cache.ttl_secs.unwrap_or(3600) as usize),
        );

        let guardrails_cfg = config.guardrails.as_ref().map(|g| GuardrailsConfig {
            max_input_chars: g.max_input_chars.unwrap_or(100_000),
            max_output_chars: g.max_output_chars.unwrap_or(100_000),
            blocked_patterns: g.blocked_patterns.clone().unwrap_or_default(),
            required_patterns: g.required_patterns.clone().unwrap_or_default(),
        }).unwrap_or_default();

        let client: Arc<dyn ProviderAdapter> = Arc::new(
            GuardrailsClient::new(client, guardrails_cfg),
        );
        client
    };

    let mut router = ProviderMap::new();

    if let Some(ref openai_cfg) = config.providers.openai {
        let client = make_pipeline(
            Arc::new(OpenAIClient::new(&openai_cfg.api_key, &openai_cfg.base_url)),
            "default",
        );
        router = router.with_provider("openai", client);
    }

    if let Some(ref anthropic_cfg) = config.providers.anthropic {
        let client = make_pipeline(
            Arc::new(AnthropicClient::new(&anthropic_cfg.api_key, &anthropic_cfg.base_url)),
            "default",
        );
        router = router.with_provider("anthropic", client);
    }

    let default = config.routing.as_ref()
        .and_then(|r| r.default_provider.as_deref())
        .unwrap_or("openai");
    router = router.with_default(default);

    Arc::new(router)
}

impl AppState {
    pub fn with_defaults(config: &GatewayConfig) -> Arc<Self> {
        let router = build_router(config);
        Arc::new(Self {
            router: Arc::new(RwLock::new(router)),
            metrics: Metrics::new(),
        })
    }

    pub fn reload_config(&self, config: GatewayConfig) {
        let new_router = build_router(&config);
        *self.router.write().unwrap() = new_router;
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

async fn handle_metrics(
    State(state): State<Arc<AppState>>,
) -> (axum::http::StatusCode, String) {
    let body = state.metrics.encode();
    (axum::http::StatusCode::OK, body)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(handle_root))
        .route("/metrics", get(handle_metrics))
        .route("/chat", post(handle_chat))
        .route("/chat/stream", post(handle_chat_stream))
        .with_state(state)
}

#[instrument(skip(state, req), fields(provider, model, latency_ms, token_count))]
async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<GatewayResponse>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let provider_name = req.provider.as_deref().unwrap_or("openai");
    let model = req.model.as_str();
    let span = tracing::Span::current();
    span.record("provider", provider_name);
    span.record("model", model);

    state.metrics.requests_in_flight.inc();
    let start = Instant::now();

    let router = state.router.read().unwrap().clone();
    let result = router.complete(&req, Some(provider_name)).await;
    let latency = start.elapsed().as_secs_f64();

    match result {
        Ok(response) => {
            let token_count = response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
            span.record("latency_ms", (latency * 1000.0) as i64);
            span.record("token_count", token_count as i64);

            state.metrics.observe_request(provider_name, model, "success", latency);
            if let Some(ref usage) = response.usage {
                state.metrics.record_tokens(provider_name, usage.prompt_tokens, usage.completion_tokens);
            }
            state.metrics.requests_in_flight.dec();

            let latency_ms = (latency * 1000.0) as u64;
            Ok(Json(GatewayResponse::from_provider(response, provider_name, latency_ms, false)))
        }
        Err(e) => {
            state.metrics.observe_request(provider_name, model, "error", latency);
            state.metrics.record_error(provider_name, &format!("{:?}", e));
            state.metrics.requests_in_flight.dec();
            Err(map_error(e))
        }
    }
}

#[instrument(skip(state, req), fields(provider, model))]
pub async fn handle_chat_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<
    Sse<impl Stream<Item = Result<Event, Infallible>>>,
    (axum::http::StatusCode, Json<serde_json::Value>),
> {
    let provider_name = req.provider.as_deref().unwrap_or("openai");
    let model = req.model.as_str();
    let span = tracing::Span::current();
    span.record("provider", provider_name);
    span.record("model", model);

    state.metrics.requests_in_flight.inc();
    let start = Instant::now();

    let router = state.router.read().unwrap().clone();

    let byte_stream = router.stream(&req, Some(provider_name)).await.map_err(|e| {
        state.metrics.observe_request(provider_name, model, "error", start.elapsed().as_secs_f64());
        state.metrics.record_error(provider_name, &format!("{:?}", e));
        state.metrics.requests_in_flight.dec();
        map_error(e)
    })?;

    let metrics = state.metrics.clone();
    let provider = provider_name.to_string();
    let model = model.to_string();

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

        let latency = start.elapsed().as_secs_f64();
        metrics.observe_request(&provider, &model, "success", latency);
        metrics.requests_in_flight.dec();
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
