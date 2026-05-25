use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::client::ProviderClient;
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse, Choice, Message, Usage};

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl ProviderClient for OpenAIClient {
    async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let openai_req = ChatRequest {
            model: req.model.clone(),
            messages: req.messages.into_iter().map(|m| Message {
                role: m.role,
                content: m.content,
            }).collect(),
        };

        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(if status == 429 {
                ProviderError::RateLimited
            } else {
                ProviderError::Api { status, body }
            });
        }

        let openai_resp: ChatResponse = resp.json().await?;

        Ok(ChatCompletionResponse {
            id: openai_resp.id,
            object: openai_resp.object,
            created: openai_resp.created,
            model: openai_resp.model,
            choices: openai_resp.choices,
            usage: openai_resp.usage,
        })
    }
}
