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

/// Service-level error that wraps inner service errors.
///
/// This error type is returned by the [`RateLimiter`](crate::RateLimiter) service and
/// allows services with any error type to be wrapped without losing inner service errors.
///
/// # Examples
///
/// ```rust
/// use tower_resilience_ratelimiter::RateLimiterServiceError;
///
/// // Match on the error to determine the cause
/// fn handle_error<E: std::fmt::Debug>(err: RateLimiterServiceError<E>) {
///     match err {
///         RateLimiterServiceError::RateLimited => {
///             println!("Rate limit exceeded");
///         }
///         RateLimiterServiceError::Inner(e) => {
///             println!("Inner service error: {:?}", e);
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub enum RateLimiterServiceError<E> {
    /// Rate limiter rejected the request.
    RateLimited,
    /// Error from the inner service.
    Inner(E),
}

impl<E> RateLimiterServiceError<E> {
    /// Returns true if this is a rate limiting error.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, RateLimiterServiceError::RateLimited)
    }

    /// Returns true if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, RateLimiterServiceError::Inner(_))
    }

    /// Converts this error into the inner error, if any.
    pub fn into_inner(self) -> Option<E> {
        match self {
            RateLimiterServiceError::RateLimited => None,
            RateLimiterServiceError::Inner(e) => Some(e),
        }
    }
}

impl<E: fmt::Display> fmt::Display for RateLimiterServiceError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateLimiterServiceError::RateLimited => write!(f, "rate limit exceeded"),
            RateLimiterServiceError::Inner(e) => write!(f, "inner service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RateLimiterServiceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RateLimiterServiceError::RateLimited => None,
            RateLimiterServiceError::Inner(e) => Some(e),
        }
    }
}

impl<E> From<RateLimiterError> for RateLimiterServiceError<E> {
    fn from(_err: RateLimiterError) -> Self {
        RateLimiterServiceError::RateLimited
    }
}

// Conversion to ResilienceError for zero-boilerplate error handling
impl<E> From<RateLimiterError> for ResilienceError<E> {
    fn from(_err: RateLimiterError) -> Self {
        ResilienceError::RateLimited { retry_after: None }
    }
}

impl<E> From<RateLimiterServiceError<E>> for ResilienceError<E> {
    fn from(err: RateLimiterServiceError<E>) -> Self {
        match err {
            RateLimiterServiceError::RateLimited => {
                ResilienceError::RateLimited { retry_after: None }
            }
            RateLimiterServiceError::Inner(e) => ResilienceError::Application(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion that RateLimiterError is Send + Sync + 'static.
    /// This is required for compatibility with tower's BoxError.
    const _: () = {
        const fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<RateLimiterError>();
    };

    #[test]
    fn test_display() {
        let error = RateLimiterError::RateLimitExceeded;
        assert_eq!(error.to_string(), "rate limit exceeded");
    }

    #[test]
    fn test_into_box_error() {
        let err = RateLimiterError::RateLimitExceeded;
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
        assert_eq!(boxed.to_string(), "rate limit exceeded");
    }
}
