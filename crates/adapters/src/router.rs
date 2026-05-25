use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

use crate::client::{ByteStream, ProviderAdapter};
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

#[derive(Clone)]
pub struct ProviderMap {
    providers: Arc<DashMap<String, Arc<dyn ProviderAdapter>>>,
    default_provider: Option<String>,
    preferred_order: Arc<Vec<String>>,
}

impl ProviderMap {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(DashMap::new()),
            default_provider: None,
            preferred_order: Arc::new(Vec::new()),
        }
    }

    pub fn with_default(mut self, provider: &str) -> Self {
        self.default_provider = Some(provider.to_string());
        self
    }

    pub fn with_provider(self, name: &str, client: Arc<dyn ProviderAdapter>) -> Self {
        self.providers.insert(name.to_string(), client);
        self
    }

    pub fn add(&self, name: &str, client: Arc<dyn ProviderAdapter>) {
        self.providers.insert(name.to_string(), client);
    }

    pub fn set_preferred_order(&mut self, order: Vec<String>) {
        self.preferred_order = Arc::new(order);
    }
}

#[async_trait]
pub trait ProviderRouter: Send + Sync {
    async fn complete(&self, req: &ChatCompletionRequest, provider: Option<&str>) -> Result<ChatCompletionResponse, ProviderError>;
    async fn stream(&self, req: &ChatCompletionRequest, provider: Option<&str>) -> Result<ByteStream, ProviderError>;
}

#[async_trait]
impl ProviderRouter for ProviderMap {
    async fn complete(&self, req: &ChatCompletionRequest, provider: Option<&str>) -> Result<ChatCompletionResponse, ProviderError> {
        let provider_name = provider
            .or(self.default_provider.as_deref())
            .unwrap_or("openai");

        if let Some(client) = self.providers.get(provider_name) {
            return client.complete(req.clone()).await;
        }

        Err(ProviderError::NoProvider)
    }

    async fn stream(&self, req: &ChatCompletionRequest, provider: Option<&str>) -> Result<ByteStream, ProviderError> {
        let provider_name = provider
            .or(self.default_provider.as_deref())
            .unwrap_or("openai");

        if let Some(client) = self.providers.get(provider_name) {
            return client.stream(req.clone()).await;
        }

        Err(ProviderError::NoProvider)
    }
}
