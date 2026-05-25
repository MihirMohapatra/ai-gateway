use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

#[async_trait]
pub trait ProviderClient: Send + Sync {
    async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError>;
}

pub struct RetryClient<T: ProviderClient> {
    inner: T,
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl<T: ProviderClient> RetryClient<T> {
    pub fn new(inner: T, max_retries: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self { inner, max_retries, base_delay, max_delay }
    }
}

#[async_trait]
impl<T: ProviderClient + Send + Sync> ProviderClient for RetryClient<T> {
    async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
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

            match self.inner.chat_completion(req.clone()).await {
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
}

#[derive(Clone, PartialEq)]
enum CircuitState {
    Closed { failure_count: u32 },
    Open { until: Instant },
    HalfOpen,
}

pub struct CircuitBreakerClient<T: ProviderClient> {
    inner: T,
    state: Arc<Mutex<CircuitState>>,
    failure_threshold: u32,
    reset_timeout: Duration,
    _half_open_max: u32,
}

impl<T: ProviderClient> CircuitBreakerClient<T> {
    pub fn new(inner: T, failure_threshold: u32, reset_timeout: Duration, half_open_max: u32) -> Self {
        Self {
            inner,
            state: Arc::new(Mutex::new(CircuitState::Closed { failure_count: 0 })),
            failure_threshold,
            reset_timeout,
            _half_open_max: half_open_max,
        }
    }
}

#[async_trait]
impl<T: ProviderClient + Send + Sync> ProviderClient for CircuitBreakerClient<T> {
    async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        {
            let mut state = self.state.lock().unwrap();
            match state.clone() {
                CircuitState::Open { until } if Instant::now() < until => {
                    return Err(ProviderError::CircuitOpen);
                }
                CircuitState::Open { .. } => {
                    *state = CircuitState::HalfOpen;
                }
                _ => {}
            }
        }

        let result = self.inner.chat_completion(req).await;

        let mut state = self.state.lock().unwrap();
        match result {
            Ok(resp) => {
                match &*state {
                    CircuitState::HalfOpen => {
                        *state = CircuitState::Closed { failure_count: 0 };
                    }
                    CircuitState::Closed { .. } => {
                        *state = CircuitState::Closed { failure_count: 0 };
                    }
                    _ => {}
                }
                Ok(resp)
            }
            Err(e) => {
                match &*state {
                    CircuitState::Closed { failure_count } => {
                        let new_count = failure_count + 1;
                        if new_count >= self.failure_threshold {
                            *state = CircuitState::Open { until: Instant::now() + self.reset_timeout };
                        } else {
                            *state = CircuitState::Closed { failure_count: new_count };
                        }
                    }
                    CircuitState::HalfOpen => {
                        *state = CircuitState::Open { until: Instant::now() + self.reset_timeout };
                    }
                    _ => {}
                }
                Err(e)
            }
        }
    }
}
