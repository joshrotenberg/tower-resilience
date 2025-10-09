use thiserror::Error;
use tower_resilience_core::ResilienceError;

/// Errors returned by the `CircuitBreaker` service.
#[derive(Debug, Error)]
pub enum CircuitBreakerError<E> {
    /// The circuit is open; calls are not permitted.
    #[error("circuit is open; call not permitted")]
    OpenCircuit,

    /// An error returned by the inner service.
    #[error("inner service error: {0}")]
    Inner(E),
}

impl<E> CircuitBreakerError<E> {
    /// Returns true if the error indicates the circuit is open.
    pub fn is_circuit_open(&self) -> bool {
        matches!(self, CircuitBreakerError::OpenCircuit)
    }

    /// Returns the inner error if present.
    pub fn into_inner(self) -> Option<E> {
        match self {
            CircuitBreakerError::Inner(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<E> for CircuitBreakerError<E> {
    fn from(err: E) -> Self {
        CircuitBreakerError::Inner(err)
    }
}

// Conversion to ResilienceError for zero-boilerplate error handling
impl<E> From<CircuitBreakerError<E>> for ResilienceError<E> {
    fn from(err: CircuitBreakerError<E>) -> Self {
        match err {
            CircuitBreakerError::OpenCircuit => ResilienceError::CircuitOpen { name: None },
            CircuitBreakerError::Inner(e) => ResilienceError::Application(e),
        }
    }
}
