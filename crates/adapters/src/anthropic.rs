use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;

use crate::client::{ByteStream, ProviderAdapter};
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse, Choice, Message, Usage};

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    pub stream: Option<bool>,
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

    fn build_request(&self, req: &ChatCompletionRequest, stream: bool) -> reqwest::RequestBuilder {
        let anthropic_req = AnthropicRequest {
            model: req.model.clone(),
            messages: req.messages.iter().map(|m| AnthropicMessage {
                role: m.role.clone(),
                content: serde_json::to_string(&m.content).unwrap_or_default(),
            }).collect(),
            max_tokens: req.max_tokens.unwrap_or(1024),
            stream: if stream { Some(true) } else { None },
        };

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&anthropic_req)
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicClient {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let resp = self.build_request(&req, false).send().await?;

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

    async fn stream(&self, req: ChatCompletionRequest) -> Result<ByteStream, ProviderError> {
        let resp = self.build_request(&req, true).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(if status == 429 {
                ProviderError::RateLimited
            } else {
                ProviderError::Api { status, body }
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, ProviderError>>(64);

        tokio::spawn(async move {
            let byte_stream = resp.bytes_stream().map(|r| {
                r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            });
            let reader = StreamReader::new(byte_stream);
            let mut lines = tokio::io::BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(data) = line.strip_prefix("data: ") {
                    let trimmed = data.trim();
                    let _ = tx.send(Ok(Bytes::from(trimmed.to_string()))).await;
                }
                if line.is_empty() {
                    let _ = tx.send(Ok(Bytes::from("\n"))).await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn token_count(&self, text: &str) -> usize {
        (text.len() / 4).max(text.split_whitespace().count())
    }
}
