use crate::config::CircuitBreakerConfig;
use crate::CircuitBreaker;
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

    /// Creates a new builder for configuring a circuit breaker layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// # type MyResponse = String;
    /// # type MyError = std::io::Error;
    /// let layer: CircuitBreakerLayer<MyResponse, MyError> = CircuitBreakerLayer::builder()
    ///     .failure_rate_threshold(0.5)
    ///     .sliding_window_size(100)
    ///     .build();
    /// ```
    pub fn builder() -> crate::CircuitBreakerConfigBuilder<Res, Err> {
        crate::CircuitBreakerConfigBuilder::new()
    }

    /// Wraps the given service with the circuit breaker middleware.
    pub fn layer<S, Req>(&self, service: S) -> CircuitBreaker<S, Req, Res, Err> {
        CircuitBreaker::new(service, self.config.clone())
    }
}

// We can't implement Layer generically because we don't know the Request type.
// The layer() method handles this by being generic over the request type.
