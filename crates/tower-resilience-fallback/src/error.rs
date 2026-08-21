//! Error types for the fallback service.

use std::fmt;

/// Error type for fallback middleware.
///
/// `E` is the primary service error. `FallbackE` is the backup service error
/// and defaults to `E` so existing value, function, and closure-backed fallback
/// APIs retain their original `FallbackError<E>` type.
///
/// Both typed errors remain available through the variants and accessor methods.
/// They are intentionally not exposed through [`std::error::Error::source`] so
/// boxed Tower errors can be wrapped directly.
#[derive(Debug)]
pub enum FallbackError<E, FallbackE = E> {
    /// The inner service failed and no fallback was applied (predicate didn't match),
    /// or the error was transformed via the exception strategy.
    Inner(E),

    /// The fallback service itself failed.
    FallbackFailed(FallbackE),
}

impl<E, FallbackE> FallbackError<E, FallbackE> {
    /// Returns `true` if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, Self::Inner(_))
    }

    /// Returns `true` if the fallback itself failed.
    pub fn is_fallback_failed(&self) -> bool {
        matches!(self, Self::FallbackFailed(_))
    }

    /// Returns the primary error when fallback was skipped.
    pub fn primary_error(&self) -> Option<&E> {
        match self {
            Self::Inner(error) => Some(error),
            Self::FallbackFailed(_) => None,
        }
    }

    /// Returns the backup service error when fallback was attempted and failed.
    pub fn fallback_error(&self) -> Option<&FallbackE> {
        match self {
            Self::Inner(_) => None,
            Self::FallbackFailed(error) => Some(error),
        }
    }

    /// Maps the primary error while preserving any backup error.
    pub fn map_primary<F, U>(self, f: F) -> FallbackError<U, FallbackE>
    where
        F: FnOnce(E) -> U,
    {
        match self {
            Self::Inner(error) => FallbackError::Inner(f(error)),
            Self::FallbackFailed(error) => FallbackError::FallbackFailed(error),
        }
    }

    /// Maps the backup error while preserving any primary error.
    pub fn map_fallback<F, U>(self, f: F) -> FallbackError<E, U>
    where
        F: FnOnce(FallbackE) -> U,
    {
        match self {
            Self::Inner(error) => FallbackError::Inner(error),
            Self::FallbackFailed(error) => FallbackError::FallbackFailed(f(error)),
        }
    }
}

impl<E> FallbackError<E, E> {
    /// Converts into the contained primary or fallback error.
    pub fn into_inner(self) -> E {
        match self {
            Self::Inner(error) | Self::FallbackFailed(error) => error,
        }
    }

    /// Returns the contained primary or fallback error.
    pub fn inner(&self) -> &E {
        match self {
            Self::Inner(error) | Self::FallbackFailed(error) => error,
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

impl<E: Clone, FallbackE: Clone> Clone for FallbackError<E, FallbackE> {
    fn clone(&self) -> Self {
        match self {
            Self::Inner(e) => Self::Inner(e.clone()),
            Self::FallbackFailed(e) => Self::FallbackFailed(e.clone()),
        }
    }
}

impl<E: fmt::Display, FallbackE: fmt::Display> fmt::Display for FallbackError<E, FallbackE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inner(e) => write!(f, "inner service error: {}", e),
            Self::FallbackFailed(e) => write!(f, "fallback failed: {}", e),
        }
    }
}

impl<E, FallbackE> std::error::Error for FallbackError<E, FallbackE>
where
    E: fmt::Debug + fmt::Display,
    FallbackE: fmt::Debug + fmt::Display,
{
}
