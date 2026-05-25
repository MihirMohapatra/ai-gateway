use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::anthropic;
use crate::openai;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: Option<bool>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub provider: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl ChatCompletionRequest {
    pub fn to_openai(&self) -> openai::ChatRequest {
        openai::ChatRequest {
            model: self.model.clone(),
            messages: self.messages.iter().map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
            }).collect(),
        }
    }

    pub fn to_anthropic(&self) -> anthropic::AnthropicRequest {
        anthropic::AnthropicRequest {
            model: self.model.clone(),
            messages: self.messages.iter().map(|m| {
                let content = serde_json::to_string(&m.content).unwrap_or_default();
                anthropic::AnthropicMessage { role: m.role.clone(), content }
            }).collect(),
            max_tokens: self.max_tokens.unwrap_or(1024),
        }
    }
}
