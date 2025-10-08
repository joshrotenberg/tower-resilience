//! Error types for cache.

use std::fmt;

/// Errors that can occur in the cache.
#[derive(Debug)]
pub enum CacheError<E> {
    /// The inner service returned an error.
    Inner(E),
}

impl<E: fmt::Display> fmt::Display for CacheError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Inner(e) => write!(f, "inner service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CacheError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::Inner(e) => Some(e),
        }
    }
}

impl<E> CacheError<E> {
    /// Converts this error into the inner error.
    pub fn into_inner(self) -> E {
        match self {
            CacheError::Inner(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inner_error() {
        let err = CacheError::Inner("test error");
        assert_eq!(err.to_string(), "inner service error: test error");
        assert_eq!(err.into_inner(), "test error");
    }
}
