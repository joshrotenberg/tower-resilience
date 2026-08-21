//! Error types for cache.

use std::fmt;

/// Errors that can occur in the cache.
///
/// The inner service error remains available through [`CacheError::Inner`] and
/// [`CacheError::into_inner`]. It is intentionally not exposed through
/// [`std::error::Error::source`] so boxed Tower errors can be wrapped directly.
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

impl<E> std::error::Error for CacheError<E> where E: fmt::Debug + fmt::Display {}

impl<E> CacheError<E> {
    /// Converts this error into the inner error.
    pub fn into_inner(self) -> E {
        match self {
            CacheError::Inner(e) => e,
        }
    }
}

/// Errors that can occur when building a cache layer.
#[derive(Debug)]
pub enum CacheBuildError {
    /// A `key_extractor` was not set before calling `build()`.
    MissingKeyExtractor,
}

impl fmt::Display for CacheBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheBuildError::MissingKeyExtractor => {
                write!(f, "key_extractor must be set before building")
            }
        }
    }
}

impl std::error::Error for CacheBuildError {}

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
