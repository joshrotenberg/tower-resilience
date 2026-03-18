use crate::circuit::Circuit;
use crate::classifier::{DefaultClassifier, FnClassifier};
use crate::config::CircuitBreakerConfig;
use crate::CircuitBreaker;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::Layer;

/// A Tower Layer that applies circuit breaker behavior to an inner service.
///
/// The type parameter `C` is the failure classifier type:
/// - `CircuitBreakerLayer<DefaultClassifier>` - uses default classification (errors = failures)
/// - `CircuitBreakerLayer<FnClassifier<F>>` - uses a custom classifier function
///
/// # Usage
///
/// ## Default Classifier (recommended for most cases)
///
/// When using the default classifier, no type parameters are needed and the layer
/// can be used directly with `ServiceBuilder`:
///
/// ```rust
/// use tower::{ServiceBuilder, service_fn};
/// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
///
/// let layer = CircuitBreakerLayer::builder()
///     .failure_rate_threshold(0.5)
///     .build();
///
/// // Works directly with ServiceBuilder - no .for_request() needed!
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) }));
/// ```
///
/// ## Custom Classifier
///
/// When you provide a custom failure classifier, the types are inferred from
/// the closure signature:
///
/// ```rust
/// use tower::{ServiceBuilder, service_fn};
/// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
/// use std::io::{Error, ErrorKind};
///
/// let layer = CircuitBreakerLayer::builder()
///     .failure_classifier(|result: &Result<String, Error>| {
///         match result {
///             Ok(_) => false,
///             Err(e) if e.kind() == ErrorKind::TimedOut => false, // Don't count timeouts
///             Err(_) => true,
///         }
///     })
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: String| async move { Ok::<_, Error>(req) }));
/// ```
/// Shared circuit state created by `build_with_handle()`.
///
/// When present, all services produced by this layer share the same circuit
/// state, and the external [`CircuitBreakerHandle`](crate::CircuitBreakerHandle)
/// can observe it.
#[derive(Clone)]
pub(crate) struct SharedCircuit {
    pub(crate) circuit: Arc<Mutex<Circuit>>,
    pub(crate) state_atomic: Arc<std::sync::atomic::AtomicU8>,
}

#[derive(Clone)]
pub struct CircuitBreakerLayer<C = DefaultClassifier> {
    config: Arc<CircuitBreakerConfig<C>>,
    /// Pre-created shared state (set by `build_with_handle()`).
    /// When `Some`, all services produced by this layer share this state.
    pub(crate) shared: Option<SharedCircuit>,
}

impl<C> CircuitBreakerLayer<C> {
    /// Creates a new `CircuitBreakerLayer` from the given configuration.
    pub(crate) fn new(config: impl Into<Arc<CircuitBreakerConfig<C>>>) -> Self {
        Self {
            config: config.into(),
            shared: None,
        }
    }

    /// Creates a new `CircuitBreakerLayer` with pre-created shared state.
    pub(crate) fn new_with_shared(
        config: impl Into<Arc<CircuitBreakerConfig<C>>>,
        shared: SharedCircuit,
    ) -> Self {
        Self {
            config: config.into(),
            shared: Some(shared),
        }
    }

    /// Creates a service from this layer, using shared state if available.
    fn make_service<S>(&self, service: S) -> CircuitBreaker<S, C> {
        if let Some(shared) = &self.shared {
            CircuitBreaker::from_shared(
                service,
                Arc::clone(&shared.circuit),
                Arc::clone(&shared.state_atomic),
                Arc::clone(&self.config),
            )
        } else {
            CircuitBreaker::new(service, Arc::clone(&self.config))
        }
    }

    /// Wraps the given service with the circuit breaker middleware.
    ///
    /// This is useful when you need direct access to the `CircuitBreaker` service,
    /// for example to call `with_fallback()` or access state inspection methods.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    /// use tower::service_fn;
    /// use futures::future::BoxFuture;
    ///
    /// # async fn example() {
    /// let layer = CircuitBreakerLayer::builder().build();
    /// let svc = service_fn(|req: String| async move { Ok::<String, ()>(req) });
    ///
    /// let mut service = layer.layer_fn(svc)
    ///     .with_fallback(|_req: String| -> BoxFuture<'static, Result<String, ()>> {
    ///         Box::pin(async { Ok("fallback".to_string()) })
    ///     });
    /// # }
    /// ```
    pub fn layer_fn<S>(&self, service: S) -> CircuitBreaker<S, C>
    where
        C: Clone,
    {
        self.make_service(service)
    }
}

impl CircuitBreakerLayer<DefaultClassifier> {
    /// Creates a new builder for configuring a circuit breaker layer.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower::{ServiceBuilder, service_fn};
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// // No type parameters needed!
    /// let layer = CircuitBreakerLayer::builder()
    ///     .failure_rate_threshold(0.5)
    ///     .sliding_window_size(100)
    ///     .build();
    ///
    /// let service = ServiceBuilder::new()
    ///     .layer(layer)
    ///     .service(service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) }));
    /// ```
    pub fn builder() -> crate::CircuitBreakerConfigBuilder<DefaultClassifier> {
        crate::CircuitBreakerConfigBuilder::new()
    }

    // =========================================================================
    // Presets
    // =========================================================================

    /// Preset: Standard balanced circuit breaker configuration.
    ///
    /// Configuration:
    /// - 50% failure rate threshold
    /// - 100 call sliding window
    /// - 30 second wait duration in open state
    /// - 3 permitted calls in half-open state
    ///
    /// This is a balanced configuration suitable for most use cases.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// // Use as-is
    /// let layer = CircuitBreakerLayer::standard().build();
    ///
    /// // Or customize further
    /// let layer = CircuitBreakerLayer::standard()
    ///     .name("my-service")
    ///     .build();
    /// ```
    pub fn standard() -> crate::CircuitBreakerConfigBuilder<DefaultClassifier> {
        use std::time::Duration;
        Self::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(100)
            .wait_duration_in_open(Duration::from_secs(30))
            .permitted_calls_in_half_open(3)
    }

    /// Preset: Fast-fail circuit breaker for latency-sensitive scenarios.
    ///
    /// Configuration:
    /// - 25% failure rate threshold (opens quickly)
    /// - 20 call sliding window (reacts faster to failures)
    /// - 10 second wait duration in open state
    /// - 1 permitted call in half-open state
    ///
    /// Use this when you want to fail fast and protect downstream services
    /// from cascading failures. Good for latency-sensitive applications.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// let layer = CircuitBreakerLayer::fast_fail().build();
    /// ```
    pub fn fast_fail() -> crate::CircuitBreakerConfigBuilder<DefaultClassifier> {
        use std::time::Duration;
        Self::builder()
            .failure_rate_threshold(0.25)
            .sliding_window_size(20)
            .wait_duration_in_open(Duration::from_secs(10))
            .permitted_calls_in_half_open(1)
    }

    /// Preset: Tolerant circuit breaker for resilient scenarios.
    ///
    /// Configuration:
    /// - 75% failure rate threshold (tolerates more failures)
    /// - 200 call sliding window (smoother failure rate)
    /// - 60 second wait duration in open state
    /// - 5 permitted calls in half-open state
    ///
    /// Use this when you want to tolerate more failures before opening,
    /// such as when calling services that occasionally have transient issues.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// let layer = CircuitBreakerLayer::tolerant().build();
    /// ```
    pub fn tolerant() -> crate::CircuitBreakerConfigBuilder<DefaultClassifier> {
        use std::time::Duration;
        Self::builder()
            .failure_rate_threshold(0.75)
            .sliding_window_size(200)
            .wait_duration_in_open(Duration::from_secs(60))
            .permitted_calls_in_half_open(5)
    }
}

// Implement Layer<S> for DefaultClassifier - works with any service
impl<S> Layer<S> for CircuitBreakerLayer<DefaultClassifier> {
    type Service = CircuitBreaker<S, DefaultClassifier>;

    fn layer(&self, service: S) -> Self::Service {
        self.make_service(service)
    }
}

// Implement Layer<S> for FnClassifier - the classifier determines compatible services
impl<S, F> Layer<S> for CircuitBreakerLayer<FnClassifier<F>> {
    type Service = CircuitBreaker<S, FnClassifier<F>>;

    fn layer(&self, service: S) -> Self::Service {
        self.make_service(service)
    }
}
