use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),
    #[error("rate limited by the source")]
    RateLimited,
    #[error("blocked by the source (captcha, 403 or anti-bot)")]
    Blocked,
    #[error("unexpected response, the source may have changed: {0}")]
    Parse(String),
    #[error("query not supported by this provider")]
    Unsupported,
}

impl ProviderError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Network(_) => "network",
            Self::RateLimited => "rate_limited",
            Self::Blocked => "blocked",
            Self::Parse(_) => "parse",
            Self::Unsupported => "unsupported",
        }
    }

    /// Errors that suggest the source changed or banned us, rather than a transient hiccup.
    pub fn is_structural(&self) -> bool {
        matches!(self, Self::Blocked | Self::Parse(_))
    }
}
