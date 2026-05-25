use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError>;
    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError>;
    fn token_count(&self, text: &str) -> usize;
}

#[async_trait]
impl<T: ProviderAdapter + ?Sized> ProviderAdapter for Arc<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        self.as_ref().complete(req).await
    }

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        self.as_ref().stream(req).await
    }

    fn token_count(&self, text: &str) -> usize {
        self.as_ref().token_count(text)
    }
}

pub struct RetryClient<T: ProviderAdapter> {
    inner: T,
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl<T: ProviderAdapter> RetryClient<T> {
    pub fn new(inner: T, max_retries: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self { inner, max_retries, base_delay, max_delay }
    }
}

#[async_trait]
impl<T: ProviderAdapter + Send + Sync> ProviderAdapter for RetryClient<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let mut last_err = ProviderError::AllProvidersFailed;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let delay = self.base_delay
                    .checked_mul(2u32.pow(attempt - 1))
                    .unwrap_or(self.max_delay)
                    .min(self.max_delay);
                tokio::time::sleep(delay).await;
                tracing::warn!(attempt, "retrying chat completion");
            }

            match self.inner.complete(req.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(ProviderError::RateLimited) | Err(ProviderError::Timeout) => {
                    last_err = ProviderError::RateLimited;
                    continue;
                }
                Err(e @ ProviderError::CircuitOpen) => return Err(e),
                Err(e @ ProviderError::NoProvider) => return Err(e),
                Err(e) => {
                    last_err = e;
                    continue;
                }
            }
        }

        Err(last_err)
    }

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        self.inner.stream(req).await
    }

    fn token_count(&self, text: &str) -> usize {
        self.inner.token_count(text)
    }
}

#[derive(Clone, PartialEq)]
enum CircuitState {
    Closed { failure_count: u32 },
    Open { until: Instant },
    HalfOpen,
}

pub struct CircuitBreakerClient<T: ProviderAdapter> {
    inner: T,
    state: Arc<Mutex<CircuitState>>,
    failure_threshold: u32,
    reset_timeout: Duration,
    _half_open_max: u32,
}

impl<T: ProviderAdapter> CircuitBreakerClient<T> {
    pub fn new(inner: T, failure_threshold: u32, reset_timeout: Duration, half_open_max: u32) -> Self {
        Self {
            inner,
            state: Arc::new(Mutex::new(CircuitState::Closed { failure_count: 0 })),
            failure_threshold,
            reset_timeout,
            _half_open_max: half_open_max,
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

    fn record_result(&self, success: bool) {
        let mut state = self.state.lock().unwrap();
        match (&*state, success) {
            (CircuitState::HalfOpen, true) => {
                *state = CircuitState::Closed { failure_count: 0 };
            }
            (CircuitState::HalfOpen, false) => {
                *state = CircuitState::Open { until: Instant::now() + self.reset_timeout };
            }
            (CircuitState::Closed { failure_count }, false) => {
                let new_count = failure_count + 1;
                if new_count >= self.failure_threshold {
                    *state = CircuitState::Open { until: Instant::now() + self.reset_timeout };
                } else {
                    *state = CircuitState::Closed { failure_count: new_count };
                }
            }
            (CircuitState::Closed { .. }, true) => {
                *state = CircuitState::Closed { failure_count: 0 };
            }
            _ => {}
        }
    }
}

#[async_trait]
impl<T: ProviderAdapter + Send + Sync> ProviderAdapter for CircuitBreakerClient<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        if !self.check_state() {
            return Err(ProviderError::CircuitOpen);
        }

        let result = self.inner.complete(req).await;
        self.record_result(result.is_ok());
        result
    }

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        if !self.check_state() {
            return Err(ProviderError::CircuitOpen);
        }

        let result = self.inner.stream(req).await;
        self.record_result(result.is_ok());
        result
    }

    fn token_count(&self, text: &str) -> usize {
        self.inner.token_count(text)
    }
}
