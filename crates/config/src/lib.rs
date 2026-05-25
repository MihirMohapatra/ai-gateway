pub mod loader;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub providers: ProvidersConfig,
    pub auth: AuthConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProvidersConfig {
    pub openai: Option<ProviderConfig>,
    pub anthropic: Option<ProviderConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProviderConfig {
    pub api_key: String,
    pub base_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    pub redis_url: Option<String>,
    pub ttl_secs: u64,
}
