use std::fmt;
use tower_resilience_core::ResilienceError;

/// Errors that can occur when using the rate limiter.
#[derive(Debug, Clone)]
pub enum RateLimiterError {
    /// The rate limit was exceeded and no permit could be acquired within the timeout.
    RateLimitExceeded,
}

impl fmt::Display for RateLimiterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateLimiterError::RateLimitExceeded => write!(f, "rate limit exceeded"),
        }
    }
}

impl std::error::Error for RateLimiterError {}

// Conversion to ResilienceError for zero-boilerplate error handling
impl<E> From<RateLimiterError> for ResilienceError<E> {
    fn from(_err: RateLimiterError) -> Self {
        ResilienceError::RateLimited { retry_after: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let error = RateLimiterError::RateLimitExceeded;
        assert_eq!(error.to_string(), "rate limit exceeded");
    }
}
