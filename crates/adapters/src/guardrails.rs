use async_trait::async_trait;
use regex::Regex;

use crate::client::{ByteStream, ProviderAdapter};
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

#[derive(Debug, Clone)]
pub struct GuardrailsConfig {
    pub blocked_patterns: Vec<String>,
    pub required_patterns: Vec<String>,
    pub max_input_chars: usize,
    pub max_output_chars: usize,
}

impl Default for GuardrailsConfig {
    fn default() -> Self {
        Self {
            blocked_patterns: Vec::new(),
            required_patterns: Vec::new(),
            max_input_chars: 100_000,
            max_output_chars: 100_000,
        }
    }
}

pub struct GuardrailsClient<T: ProviderAdapter> {
    inner: T,
    config: GuardrailsConfig,
}

impl<T: ProviderAdapter> GuardrailsClient<T> {
    pub fn new(inner: T, config: GuardrailsConfig) -> Self {
        Self { inner, config }
    }

    fn check_patterns(&self, text: &str, patterns: &[String], label: &str) -> Result<(), ProviderError> {
        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(text) {
                    return Err(ProviderError::GuardrailsViolation(
                        format!("{label}: matched pattern \"{pattern}\""),
                    ));
                }
            } else {
                tracing::warn!(pattern, "invalid guardrails regex");
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<T: ProviderAdapter + Send + Sync> ProviderAdapter for GuardrailsClient<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        for msg in &req.messages {
            if let Some(text) = msg.content.as_str() {
                if text.len() > self.config.max_input_chars {
                    return Err(ProviderError::GuardrailsViolation(
                        "max_input_chars exceeded".into(),
                    ));
                }
                self.check_patterns(text, &self.config.blocked_patterns, "input")?;
            }
        }

        let resp = self.inner.complete(req).await?;

        if let Some(text) = resp.choices.first().and_then(|c| c.message.content.as_str()) {
            if text.len() > self.config.max_output_chars {
                return Err(ProviderError::GuardrailsViolation(
                    "max_output_chars exceeded".into(),
                ));
            }
            self.check_patterns(text, &self.config.required_patterns, "output")?;
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
