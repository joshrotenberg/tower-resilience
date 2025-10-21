use crate::config::CircuitBreakerConfig;
use crate::CircuitBreaker;
use std::marker::PhantomData;
use std::sync::Arc;
use tower::Layer;
use tower::Service;

/// A Tower Layer that applies circuit breaker behavior to an inner service.
///
/// Wraps an inner service and manages its state according to circuit breaker logic.
#[derive(Clone)]
pub struct CircuitBreakerLayer<Res, Err> {
    config: Arc<CircuitBreakerConfig<Res, Err>>,
}

/// Request-typed circuit breaker layer that integrates with [`tower::ServiceBuilder`].
///
/// This layer carries the request type parameter `Req` needed for Tower's `Layer` trait
/// implementation, allowing it to work seamlessly with `ServiceBuilder`.
///
/// Use [`CircuitBreakerLayer::for_request`] to create this from a base layer.
#[derive(Clone)]
pub struct CircuitBreakerRequestLayer<Req, Res, Err> {
    config: Arc<CircuitBreakerConfig<Res, Err>>,
    /// PhantomData with fn() -> Req ensures covariance over Req, which is safe since
    /// we never actually store Req values - only use it in type signatures.
    /// Using fn() -> Req instead of Req makes the type covariant.
    _phantom: PhantomData<fn() -> Req>,
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
    /// use tower::{ServiceBuilder, service_fn};
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// # type MyResponse = String;
    /// # type MyError = std::io::Error;
    /// let layer: CircuitBreakerLayer<MyResponse, MyError> = CircuitBreakerLayer::builder()
    ///     .failure_rate_threshold(0.5)
    ///     .sliding_window_size(100)
    ///     .build();
    ///
    /// let service = ServiceBuilder::new()
    ///     .layer(layer.for_request::<String>())
    ///     .service(service_fn(|req: String| async move { Ok::<_, MyError>(req) }));
    /// ```
    pub fn builder() -> crate::CircuitBreakerConfigBuilder<Res, Err> {
        crate::CircuitBreakerConfigBuilder::new()
    }

    /// Converts this layer into a request-typed layer that implements [`Layer`].
    ///
    /// This enables ergonomic integration with `tower::ServiceBuilder`.
    pub fn for_request<Req>(&self) -> CircuitBreakerRequestLayer<Req, Res, Err> {
        CircuitBreakerRequestLayer {
            config: Arc::clone(&self.config),
            _phantom: PhantomData,
        }
    }

    /// Wraps the given service with the circuit breaker middleware.
    pub fn layer<S, Req>(&self, service: S) -> CircuitBreaker<S, Req, Res, Err> {
        CircuitBreaker::new(service, Arc::clone(&self.config))
    }
}

impl<Req, Res, Err> CircuitBreakerRequestLayer<Req, Res, Err> {
    fn apply<S>(&self, service: S) -> CircuitBreaker<S, Req, Res, Err>
    where
        S: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
        S::Future: Send + 'static,
        Res: Send + 'static,
        Err: Send + 'static,
        Req: Send + 'static,
    {
        CircuitBreaker::new(service, Arc::clone(&self.config))
    }
}

impl<S, Req, Res, Err> Layer<S> for CircuitBreakerRequestLayer<Req, Res, Err>
where
    S: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
    Req: Send + 'static,
{
    type Service = CircuitBreaker<S, Req, Res, Err>;

    fn layer(&self, service: S) -> Self::Service {
        self.apply(service)
    }
}
