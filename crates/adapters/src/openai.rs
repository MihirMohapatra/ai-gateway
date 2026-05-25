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
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: Option<bool>,
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

    fn build_request(&self, req: &ChatCompletionRequest, stream: bool) -> reqwest::RequestBuilder {
        let openai_req = ChatRequest {
            model: req.model.clone(),
            messages: req.messages.iter().map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
            }).collect(),
            stream: if stream { Some(true) } else { None },
        };

        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_req)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAIClient {
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
                    if trimmed == "[DONE]" {
                        continue;
                    }
                    let _ = tx.send(Ok(Bytes::from(trimmed.to_string()))).await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn token_count(&self, text: &str) -> usize {
        (text.len() / 4).max(text.split_whitespace().count())
    }
}
