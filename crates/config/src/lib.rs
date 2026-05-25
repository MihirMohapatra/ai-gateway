pub mod loader;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub providers: ProvidersConfig,
    pub auth: AuthConfig,
    pub cache: CacheConfig,
    pub guardrails: Option<GuardrailsConfig>,
    pub metering: Option<MeteringConfig>,
    pub routing: Option<RoutingConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 3000,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
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
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    pub redis_url: Option<String>,
    pub ttl_secs: Option<u64>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            redis_url: None,
            ttl_secs: Some(3600),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct GuardrailsConfig {
    pub max_input_chars: Option<usize>,
    pub max_output_chars: Option<usize>,
    pub blocked_patterns: Option<Vec<String>>,
    pub required_patterns: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MeteringConfig {
    pub database_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoutingConfig {
    pub default_provider: Option<String>,
    pub models: Option<std::collections::HashMap<String, String>>,
}
