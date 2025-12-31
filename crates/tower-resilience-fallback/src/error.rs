//! Error types for the fallback service.

use std::fmt;

/// Error type for the fallback service.
#[derive(Debug)]
pub enum FallbackError<E> {
    /// The inner service failed and no fallback was applied (predicate didn't match),
    /// or the error was transformed via the exception strategy.
    Inner(E),

    /// The fallback service itself failed.
    FallbackFailed(E),
}

impl<E> FallbackError<E> {
    /// Returns `true` if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, Self::Inner(_))
    }

    /// Returns `true` if the fallback itself failed.
    pub fn is_fallback_failed(&self) -> bool {
        matches!(self, Self::FallbackFailed(_))
    }

    /// Converts into the inner error.
    pub fn into_inner(self) -> E {
        match self {
            Self::Inner(e) | Self::FallbackFailed(e) => e,
        }
    }

    /// Returns a reference to the inner error.
    pub fn inner(&self) -> &E {
        match self {
            Self::Inner(e) | Self::FallbackFailed(e) => e,
        }
    }

    /// Maps the inner error using the provided function.
    pub fn map<F, U>(self, f: F) -> FallbackError<U>
    where
        F: FnOnce(E) -> U,
    {
        match self {
            Self::Inner(e) => FallbackError::Inner(f(e)),
            Self::FallbackFailed(e) => FallbackError::FallbackFailed(f(e)),
        }
    }
}

impl<E: Clone> Clone for FallbackError<E> {
    fn clone(&self) -> Self {
        match self {
            Self::Inner(e) => Self::Inner(e.clone()),
            Self::FallbackFailed(e) => Self::FallbackFailed(e.clone()),
        }
    }
}

impl<E: fmt::Display> fmt::Display for FallbackError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inner(e) => write!(f, "inner service error: {}", e),
            Self::FallbackFailed(e) => write!(f, "fallback failed: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FallbackError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inner(e) | Self::FallbackFailed(e) => Some(e),
        }
    }
}
