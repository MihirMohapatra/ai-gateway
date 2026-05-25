use std::sync::Arc;

use async_trait::async_trait;

use crate::client::{ByteStream, ProviderAdapter};
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

#[async_trait]
pub trait MeterBackend: Send + Sync {
    async fn record_usage(
        &self,
        api_key: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> Result<(), ProviderError>;
}

pub struct ConsoleMeter;

#[async_trait]
impl MeterBackend for ConsoleMeter {
    async fn record_usage(
        &self,
        api_key: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> Result<(), ProviderError> {
        tracing::info!(
            api_key,
            model,
            prompt_tokens,
            completion_tokens,
            "usage recorded"
        );
        Ok(())
    }
}

pub struct MeteringClient<T: ProviderAdapter> {
    inner: T,
    backend: Arc<dyn MeterBackend>,
    api_key: String,
}

impl<T: ProviderAdapter> MeteringClient<T> {
    pub fn new(inner: T, backend: Arc<dyn MeterBackend>, api_key: impl Into<String>) -> Self {
        Self { inner, backend, api_key: api_key.into() }
    }
}

#[async_trait]
impl<T: ProviderAdapter + Send + Sync> ProviderAdapter for MeteringClient<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let resp = self.inner.complete(req).await?;

        if let Some(ref usage) = resp.usage {
            let _ = self.backend
                .record_usage(&self.api_key, &resp.model, usage.prompt_tokens, usage.completion_tokens)
                .await;
        }

        Ok(resp)
    }

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        self.inner.stream(req).await
    }

    fn token_count(&self, text: &str) -> usize {
        self.inner.token_count(text)
    }
}
