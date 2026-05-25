use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::client::ProviderClient;
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse, Choice, Message, Usage};

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub struct AnthropicClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl ProviderClient for AnthropicClient {
    async fn chat_completion(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let anthropic_req = AnthropicRequest {
            model: req.model.clone(),
            messages: req.messages.into_iter().map(|m| AnthropicMessage {
                role: m.role,
                content: serde_json::to_string(&m.content).unwrap_or_default(),
            }).collect(),
            max_tokens: req.max_tokens.unwrap_or(1024),
        };

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let resp = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&anthropic_req)
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

        let anthropic_resp: AnthropicResponse = resp.json().await?;

        let text = anthropic_resp.content
            .into_iter()
            .find(|b| b.block_type == "text")
            .map(|b| b.text)
            .unwrap_or_default();

        Ok(ChatCompletionResponse {
            id: anthropic_resp.id,
            object: "chat.completion".into(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            model: anthropic_resp.model,
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".into(),
                    content: serde_json::Value::String(text),
                },
                finish_reason: Some("stop".into()),
            }],
            usage: anthropic_resp.usage.map(|u| Usage {
                prompt_tokens: u.input_tokens,
                completion_tokens: u.output_tokens,
                total_tokens: u.input_tokens + u.output_tokens,
            }),
        })
    }
}
