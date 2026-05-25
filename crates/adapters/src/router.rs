use std::sync::Arc;

use crate::client::ProviderClient;
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};
use dashmap::DashMap;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ProviderId {
    OpenAI,
    Anthropic,
}

pub trait ModelRouter: Send + Sync {
    fn resolve(&self, model: &str) -> Vec<ProviderId>;
}

pub struct ModelNameRouter;

impl ModelRouter for ModelNameRouter {
    fn resolve(&self, model: &str) -> Vec<ProviderId> {
        let lower = model.to_lowercase();
        if lower.starts_with("gpt") || lower.starts_with("o1") || lower.starts_with("o3") || lower.contains("openai") {
            vec![ProviderId::OpenAI, ProviderId::Anthropic]
        } else if lower.starts_with("claude") || lower.contains("anthropic") {
            vec![ProviderId::Anthropic, ProviderId::OpenAI]
        } else {
            vec![ProviderId::OpenAI, ProviderId::Anthropic]
        }
    }
}

pub struct PriorityRouter {
    providers: Vec<ProviderId>,
}

impl PriorityRouter {
    pub fn new(providers: Vec<ProviderId>) -> Self {
        Self { providers }
    }
}

impl ModelRouter for PriorityRouter {
    fn resolve(&self, _model: &str) -> Vec<ProviderId> {
        self.providers.clone()
    }
}

pub struct ProviderMap {
    clients: DashMap<ProviderId, Arc<dyn ProviderClient>>,
    router: Box<dyn ModelRouter>,
}

impl ProviderMap {
    pub fn new(router: Box<dyn ModelRouter>) -> Self {
        Self {
            clients: DashMap::new(),
            router,
        }
    }

    pub fn register(&self, id: ProviderId, client: Arc<dyn ProviderClient>) {
        self.clients.insert(id, client);
    }

    pub fn resolve(&self, model: &str) -> Vec<ProviderId> {
        self.router.resolve(model)
    }

    pub fn route(&self, _req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        Err(ProviderError::NoProvider)
    }

    pub async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let providers = self.router.resolve(&req.model);
        for provider in &providers {
            if let Some(client) = self.clients.get(provider) {
                match client.chat_completion(req.clone()).await {
                    Ok(resp) => return Ok(resp),
                    Err(e) => {
                        tracing::warn!(?provider, error = %e, "provider failed, trying next");
                        continue;
                    }
                }
            }
        }
        Err(ProviderError::AllProvidersFailed)
    }
}
