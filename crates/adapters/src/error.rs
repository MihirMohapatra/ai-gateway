#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API returned error {status}: {body}")]
    Api { status: u16, body: String },

    #[error("Rate limited by provider")]
    RateLimited,

    #[error("Request timed out")]
    Timeout,

    #[error("Circuit breaker is open")]
    CircuitOpen,

    #[error("No provider available for model")]
    NoProvider,

    #[error("All providers in fallback chain failed")]
    AllProvidersFailed,
}
