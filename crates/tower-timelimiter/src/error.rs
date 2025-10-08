//! Error types for time limiter.

use std::fmt;

/// Errors that can occur in the time limiter.
#[derive(Debug)]
pub enum TimeLimiterError<E> {
    /// The request timed out.
    Timeout,
    /// The inner service returned an error.
    Inner(E),
}

impl<E: fmt::Display> fmt::Display for TimeLimiterError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeLimiterError::Timeout => write!(f, "request timed out"),
            TimeLimiterError::Inner(e) => write!(f, "inner service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for TimeLimiterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TimeLimiterError::Timeout => None,
            TimeLimiterError::Inner(e) => Some(e),
        }
    }
}

impl<E> TimeLimiterError<E> {
    /// Returns true if this is a timeout error.
    pub fn is_timeout(&self) -> bool {
        matches!(self, TimeLimiterError::Timeout)
    }

    /// Converts this error into the inner error, if any.
    pub fn into_inner(self) -> Option<E> {
        match self {
            TimeLimiterError::Timeout => None,
            TimeLimiterError::Inner(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_error() {
        let err: TimeLimiterError<&str> = TimeLimiterError::Timeout;
        assert!(err.is_timeout());
        assert_eq!(err.into_inner(), None);
    }

    #[test]
    fn test_inner_error() {
        let err = TimeLimiterError::Inner("inner error");
        assert!(!err.is_timeout());
        assert_eq!(err.into_inner(), Some("inner error"));
    }

    #[test]
    fn test_error_display() {
        let err: TimeLimiterError<&str> = TimeLimiterError::Timeout;
        assert_eq!(err.to_string(), "request timed out");

        let err = TimeLimiterError::Inner("test");
        assert_eq!(err.to_string(), "inner service error: test");
    }
}
