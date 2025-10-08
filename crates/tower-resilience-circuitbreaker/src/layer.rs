use crate::CircuitBreaker;
use crate::config::CircuitBreakerConfig;
use std::sync::Arc;

/// A Tower Layer that applies circuit breaker behavior to an inner service.
///
/// Wraps an inner service and manages its state according to circuit breaker logic.
#[derive(Clone)]
pub struct CircuitBreakerLayer<Res, Err> {
    config: Arc<CircuitBreakerConfig<Res, Err>>,
}

impl<Res, Err> CircuitBreakerLayer<Res, Err> {
    /// Creates a new `CircuitBreakerLayer` from the given configuration.
    pub(crate) fn new(config: impl Into<Arc<CircuitBreakerConfig<Res, Err>>>) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Wraps the given service with the circuit breaker middleware.
    pub fn layer<S, Req>(&self, service: S) -> CircuitBreaker<S, Req, Res, Err> {
        CircuitBreaker::new(service, self.config.clone())
    }
}

// We can't implement Layer generically because we don't know the Request type.
// The layer() method handles this by being generic over the request type.
