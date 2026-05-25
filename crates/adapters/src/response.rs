use serde::Serialize;

use crate::request::{ChatCompletionResponse, Usage};

#[derive(Debug, Clone, Serialize)]
pub struct GatewayResponse {
    pub id: String,
    pub model: String,
    pub provider: String,
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<UsageInfo>,
    pub latency_ms: u64,
    pub cached: bool,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl From<Usage> for UsageInfo {
    fn from(u: Usage) -> Self {
        UsageInfo {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }
    }
}

impl GatewayResponse {
    pub fn from_provider(resp: ChatCompletionResponse, provider: &str, latency_ms: u64, cached: bool) -> Self {
        let content = resp.choices
            .first()
            .and_then(|c| c.message.content.as_str())
            .unwrap_or("")
            .to_string();

        Self {
            id: resp.id,
            model: resp.model,
            provider: provider.to_string(),
            content,
            finish_reason: resp.choices.first().and_then(|c| c.finish_reason.clone()),
            usage: resp.usage.map(UsageInfo::from),
            latency_ms,
            cached,
            created: resp.created,
        }
    }
}
