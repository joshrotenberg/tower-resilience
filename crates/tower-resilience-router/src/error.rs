//! Error types for the weighted router.

use std::fmt;
use tower_resilience_core::ResilienceError;

/// Errors that can occur in the weighted router.
///
/// The weighted router itself does not produce errors -- it delegates
/// to the selected backend. This error type wraps the inner service
/// error for consistency with other tower-resilience patterns.
#[derive(Debug)]
pub enum WeightedRouterError<E> {
    /// An error from the selected backend service.
    Inner(E),
}

impl<E> WeightedRouterError<E> {
    /// Returns `true` if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, WeightedRouterError::Inner(_))
    }

    /// Consumes self and returns the inner error.
    pub fn into_inner(self) -> E {
        match self {
            WeightedRouterError::Inner(e) => e,
        }
    }
}

impl<E: fmt::Display> fmt::Display for WeightedRouterError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeightedRouterError::Inner(e) => write!(f, "backend service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for WeightedRouterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WeightedRouterError::Inner(e) => Some(e),
        }
    }
}

impl<E> From<WeightedRouterError<E>> for ResilienceError<E> {
    fn from(err: WeightedRouterError<E>) -> Self {
        match err {
            WeightedRouterError::Inner(e) => ResilienceError::Application(e),
        }
    }
}

impl<E> From<WeightedRouterError<ResilienceError<E>>> for ResilienceError<E> {
    fn from(err: WeightedRouterError<ResilienceError<E>>) -> Self {
        match err {
            WeightedRouterError::Inner(re) => re,
        }
    }
}
