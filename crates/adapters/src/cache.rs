use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::client::{ByteStream, ProviderAdapter};
use crate::error::ProviderError;
use crate::request::{ChatCompletionRequest, ChatCompletionResponse};

#[async_trait]
pub trait CacheBackend: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ProviderError>;
    async fn set(&self, key: &str, value: Vec<u8>, ttl_secs: usize) -> Result<(), ProviderError>;
}

pub fn cache_key(req: &ChatCompletionRequest) -> String {
    let input = serde_json::to_string(req).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct NoopCache;

#[async_trait]
impl CacheBackend for NoopCache {
    async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, ProviderError> {
        Ok(None)
    }

    async fn set(&self, _key: &str, _value: Vec<u8>, _ttl_secs: usize) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[async_trait]
impl CacheBackend for cache::local::LocalCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ProviderError> {
        Ok(cache::local::LocalCache::get(self, key))
    }

    async fn set(&self, key: &str, value: Vec<u8>, _ttl_secs: usize) -> Result<(), ProviderError> {
        cache::local::LocalCache::set(self, key.to_string(), value);
        Ok(())
    }
}

#[async_trait]
impl CacheBackend for cache::redis::RedisCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ProviderError> {
        cache::redis::RedisCache::get(self, key)
            .await
            .map_err(|e| ProviderError::CacheError(e.to_string()))
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl_secs: usize) -> Result<(), ProviderError> {
        cache::redis::RedisCache::set(self, key, value, ttl_secs)
            .await
            .map_err(|e| ProviderError::CacheError(e.to_string()))
    }
}

pub struct CachingClient<T: ProviderAdapter> {
    inner: T,
    backend: Arc<dyn CacheBackend>,
    ttl_secs: usize,
}

impl<T: ProviderAdapter> CachingClient<T> {
    pub fn new(inner: T, backend: Arc<dyn CacheBackend>, ttl_secs: usize) -> Self {
        Self { inner, backend, ttl_secs }
    }
}

#[async_trait]
impl<T: ProviderAdapter + Send + Sync> ProviderAdapter for CachingClient<T> {
    async fn complete(&self, req: ChatCompletionRequest) -> Result<ChatCompletionResponse, ProviderError> {
        let key = cache_key(&req);

        if let Some(cached) = self.backend.get(&key).await? {
            if let Ok(resp) = serde_json::from_slice::<ChatCompletionResponse>(&cached) {
                tracing::debug!("cache hit for key {}", &key[..16]);
                return Ok(resp);
            }
        }

        let resp = self.inner.complete(req).await?;

        if let Ok(encoded) = serde_json::to_vec(&resp) {
            let _ = self.backend.set(&key, encoded, self.ttl_secs).await;
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
