//! Circuit breaker pattern for Tower services.
//!
//! A circuit breaker prevents cascading failures by monitoring service calls and
//! temporarily blocking requests when the failure rate exceeds a threshold.
//!
//! ## States
//! - **Closed**: Normal operation, all requests pass through
//! - **Open**: Circuit is tripped, requests are rejected immediately
//! - **Half-Open**: Testing if service has recovered, limited requests allowed
//!
//! ## Usage
//!
//! ### Basic Usage with ServiceBuilder
//!
//! The circuit breaker layer can be used directly with `ServiceBuilder` without
//! any type parameters:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::{ServiceBuilder, service_fn};
//!
//! # async fn example() {
//! // No type parameters needed!
//! let circuit_breaker = CircuitBreakerLayer::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(circuit_breaker)
//!     .service(service_fn(|req: String| async move {
//!         Ok::<String, std::io::Error>(req)
//!     }));
//! # }
//! ```
//!
//! ### With Fallback Handler
//!
//! Use `layer_fn()` to get access to the `CircuitBreaker` service for setting a fallback:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::service_fn;
//! use futures::future::BoxFuture;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder()
//!     .failure_rate_threshold(0.5)
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//!
//! let mut service = layer.layer_fn(svc)
//!     .with_fallback(|_req: String| -> BoxFuture<'static, Result<String, ()>> {
//!         Box::pin(async { Ok("fallback".to_string()) })
//!     });
//! # }
//! ```
//!
//! ### Custom Failure Classification
//!
//! By default, all errors are counted as failures. You can customize this:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::{ServiceBuilder, service_fn};
//! use std::io::{Error, ErrorKind};
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder()
//!     .failure_classifier(|result: &Result<String, Error>| {
//!         match result {
//!             Ok(_) => false,
//!             // Don't count timeouts as failures
//!             Err(e) if e.kind() == ErrorKind::TimedOut => false,
//!             Err(_) => true,
//!         }
//!     })
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service(service_fn(|req: String| async move { Ok::<_, Error>(req) }));
//! # }
//! ```
//!
//! ### Services with `Error = Infallible`
//!
//! Many services encode errors in the response body rather than returning `Err`:
//! - HTTP services returning error status codes as `Ok(Response)`
//! - gRPC services with status codes in the response
//! - MCP servers returning `JsonRpcResponse` with error fields
//!
//! Use `classify_response()` for these services:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//!
//! # struct Response { status_code: u16 }
//! # impl Response { fn status(&self) -> u16 { self.status_code } }
//!
//! // Classify failures based on response content
//! let layer = CircuitBreakerLayer::builder()
//!     .classify_response(|response: &Response| response.status() >= 500)
//!     .build();
//! ```
//!
//! This is simpler than `failure_classifier()` because you don't need to handle
//! the `Err` case (which can never occur with `Error = Infallible`).
//!
//! ## Time-Based Sliding Window
//!
//! Use time-based windows instead of count-based:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, SlidingWindowType};
//! use tower::{ServiceBuilder, service_fn};
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_type(SlidingWindowType::TimeBased)
//!     .sliding_window_duration(Duration::from_secs(60))  // Last 60 seconds
//!     .minimum_number_of_calls(10)
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
//! # }
//! ```
//!
//! ## Slow Call Detection
//!
//! Open circuit based on slow calls:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::{ServiceBuilder, service_fn};
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder()
//!     .failure_rate_threshold(1.0)  // Don't open on failures
//!     .slow_call_duration_threshold(Duration::from_secs(2))
//!     .slow_call_rate_threshold(0.5)  // Open at 50% slow calls
//!     .sliding_window_size(100)
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
//! # }
//! ```
//!
//! ## Event Listeners
//!
//! Monitor circuit breaker behavior:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::{ServiceBuilder, service_fn};
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .on_state_transition(|from, to| {
//!         println!("Circuit breaker: {:?} -> {:?}", from, to);
//!     })
//!     .on_call_permitted(|state| {
//!         println!("Call permitted in state: {:?}", state);
//!     })
//!     .on_call_rejected(|| {
//!         println!("Call rejected - circuit open");
//!     })
//!     .on_slow_call(|duration| {
//!         println!("Slow call detected: {:?}", duration);
//!     })
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
//! # }
//! ```
//!
//! ## Error Handling
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreakerError};
//! use tower::{Service, ServiceBuilder, service_fn};
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder().build();
//! let mut service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
//!
//! match service.call("request".to_string()).await {
//!     Ok(response) => println!("Success: {}", response),
//!     Err(CircuitBreakerError::OpenCircuit) => {
//!         eprintln!("Circuit breaker is open");
//!     }
//!     Err(CircuitBreakerError::Inner(e)) => {
//!         eprintln!("Service error: {:?}", e);
//!     }
//! }
//! # }
//! ```
//!
//! ## State Inspection and Observability
//!
//! The circuit breaker provides both synchronous and asynchronous methods for
//! inspecting state and metrics:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitState};
//! use tower::service_fn;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder().build();
//! let svc = service_fn(|req: String| async move { Ok::<String, ()>(req) });
//! let breaker = layer.layer_fn(svc);
//!
//! // Sync state inspection (no await needed, lock-free)
//! match breaker.state_sync() {
//!     CircuitState::Closed => println!("Healthy"),
//!     CircuitState::Open => println!("Circuit open - return 503"),
//!     CircuitState::HalfOpen => println!("Recovering"),
//! }
//!
//! // Convenience method
//! if breaker.is_open() {
//!     // Return error response
//! }
//!
//! // Detailed metrics (async, requires lock)
//! let metrics = breaker.metrics().await;
//! println!("Failure rate: {:.1}%", metrics.failure_rate * 100.0);
//! println!("Total calls: {}", metrics.total_calls);
//! # }
//! ```
//!
//! ## Health Check Integration
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::service_fn;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::builder().build();
//! let svc = service_fn(|req: String| async move { Ok::<String, ()>(req) });
//! let breaker = layer.layer_fn(svc);
//!
//! // Simple health status
//! let status = breaker.health_status(); // "healthy", "degraded", or "unhealthy"
//!
//! // HTTP status code for health endpoints
//! let http_status = breaker.http_status(); // 200 or 503
//! # }
//! ```
//!
//! ## Features
//! - Count-based and time-based sliding windows
//! - Configurable failure rate threshold
//! - Slow call detection and rate threshold
//! - Half-open state for gradual recovery
//! - Event system for observability
//! - Optional fallback handling
//! - Manual state control (force_open, force_closed, reset)
//! - Sync state inspection with `state_sync()`, `is_open()`, and `metrics()`
//! - Metrics integration via `metrics` feature
//! - Tracing support via `tracing` feature
//!
//! ## Feature Flags
//! - `metrics`: enables metrics collection using the `metrics` crate
//! - `tracing`: enables logging and tracing using the `tracing` crate
//! - `serde`: enables `Serialize` for `CircuitState` and `CircuitMetrics`
//!
//! ## Examples
//!
//! See the `examples/` directory for complete working examples:
//! - `circuitbreaker_example.rs` - Basic usage with state transitions
//! - `circuitbreaker_fallback.rs` - Fallback strategies for graceful degradation
//! - `circuitbreaker_health_check.rs` - Health check endpoints and monitoring

use crate::circuit::Circuit;
use crate::classifier::FailureClassifier;
use futures::future::BoxFuture;
#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter, describe_gauge, describe_histogram};
use std::sync::Arc;
#[cfg(feature = "metrics")]
use std::sync::Once;
use std::task::{Context, Poll};
use tokio::sync::Mutex;
use tower::Service;
#[cfg(feature = "tracing")]
use tracing::debug;

pub use circuit::{CircuitMetrics, CircuitState};
pub use classifier::{
    DefaultClassifier, FailureClassifier as FailureClassifierTrait, FnClassifier,
};
pub use config::{CircuitBreakerConfig, CircuitBreakerConfigBuilder, SlidingWindowType};
pub use error::CircuitBreakerError;
pub use events::CircuitBreakerEvent;
#[allow(deprecated)]
pub use layer::{CircuitBreakerLayer, CircuitBreakerRequestLayer};

mod circuit;
pub mod classifier;
mod config;
mod error;
mod events;
#[cfg(feature = "health-integration")]
mod health_integration;
mod layer;

pub(crate) type FallbackFn<Req, Res, Err> =
    dyn Fn(Req) -> BoxFuture<'static, Result<Res, Err>> + Send + Sync;
pub(crate) type SharedFallback<Req, Res, Err> = Arc<FallbackFn<Req, Res, Err>>;

#[cfg(feature = "metrics")]
static METRICS_INIT: Once = Once::new();

/// Returns a new builder for a `CircuitBreakerLayer`.
///
/// This is a convenience function that returns a builder. You can also use
/// `CircuitBreakerLayer::builder()` directly.
///
/// # Example
///
/// ```rust
/// use tower_resilience_circuitbreaker::circuit_breaker_builder;
/// use tower::{ServiceBuilder, service_fn};
///
/// let layer = circuit_breaker_builder()
///     .failure_rate_threshold(0.5)
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
/// ```
pub fn circuit_breaker_builder() -> CircuitBreakerConfigBuilder<DefaultClassifier> {
    #[cfg(feature = "metrics")]
    {
        METRICS_INIT.call_once(|| {
            describe_counter!(
                "circuitbreaker_calls_total",
                "Total number of calls through the circuit breaker"
            );
            describe_counter!(
                "circuitbreaker_transitions_total",
                "Total number of circuit breaker state transitions"
            );
            describe_counter!(
                "circuitbreaker_slow_calls_total",
                "Total number of slow calls detected"
            );
            describe_gauge!(
                "circuitbreaker_state",
                "Current state of the circuit breaker"
            );
            describe_histogram!(
                "circuitbreaker_call_duration_seconds",
                "Duration of calls through the circuit breaker"
            );
        });
    }
    CircuitBreakerConfigBuilder::default()
}

/// A Tower Service that applies circuit breaker logic to an inner service.
///
/// Manages the circuit state and controls calls to the inner service accordingly.
///
/// # Type Parameters
///
/// - `S`: The inner service type
/// - `C`: The failure classifier type (e.g., `DefaultClassifier` or `FnClassifier<F>`)
pub struct CircuitBreaker<S, C> {
    inner: S,
    pub(crate) circuit: Arc<Mutex<Circuit>>,
    state_atomic: Arc<std::sync::atomic::AtomicU8>,
    pub(crate) config: Arc<CircuitBreakerConfig<C>>,
}

impl<S, C> CircuitBreaker<S, C> {
    /// Creates a new `CircuitBreaker` wrapping the given service and configuration.
    pub(crate) fn new(inner: S, config: Arc<CircuitBreakerConfig<C>>) -> Self {
        let state_atomic = Arc::new(std::sync::atomic::AtomicU8::new(CircuitState::Closed as u8));
        Self {
            inner,
            circuit: Arc::new(Mutex::new(Circuit::new_with_atomic(Arc::clone(
                &state_atomic,
            )))),
            state_atomic,
            config,
        }
    }

    /// Sets a fallback function to be called when the circuit is open.
    ///
    /// When the circuit breaker is in the open state, instead of immediately failing requests,
    /// it will call the provided fallback function to generate an alternative response. This
    /// enables graceful degradation patterns.
    ///
    /// Returns a `CircuitBreakerWithFallback` which also implements `Service`.
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
    /// let svc = service_fn(|req: String| async move { Ok::<String, String>(req) });
    ///
    /// let mut service = layer.layer_fn(svc).with_fallback(|_req: String| {
    ///     Box::pin(async {
    ///         Ok::<String, String>("Service temporarily unavailable".to_string())
    ///     })
    /// });
    /// # }
    /// ```
    ///
    /// # Note
    ///
    /// The fallback is only called when the circuit is **open**. When closed or half-open,
    /// requests are forwarded to the inner service normally.
    pub fn with_fallback<Req, Res, Err, F>(
        self,
        fallback: F,
    ) -> CircuitBreakerWithFallback<S, C, Req, Res, Err>
    where
        F: Fn(Req) -> BoxFuture<'static, Result<Res, Err>> + Send + Sync + 'static,
    {
        CircuitBreakerWithFallback {
            inner: self.inner,
            circuit: self.circuit,
            state_atomic: self.state_atomic,
            config: self.config,
            fallback: Arc::new(fallback),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Forces the circuit into the open state.
    pub async fn force_open(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_open(&self.config);
    }

    /// Forces the circuit into the closed state.
    pub async fn force_closed(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_closed(&self.config);
    }

    /// Returns whether the circuit is currently open.
    ///
    /// This is a convenience method equivalent to `self.state_sync() == CircuitState::Open`.
    pub fn is_open(&self) -> bool {
        self.state_sync() == CircuitState::Open
    }

    /// Returns a snapshot of the current circuit breaker metrics.
    pub async fn metrics(&self) -> crate::circuit::CircuitMetrics {
        let circuit = self.circuit.lock().await;
        circuit.metrics(&self.config)
    }

    /// Resets the circuit to the closed state and clears counts.
    pub async fn reset(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.reset(&self.config);
    }

    /// Returns the current state of the circuit.
    pub async fn state(&self) -> CircuitState {
        let circuit = self.circuit.lock().await;
        circuit.state()
    }

    /// Returns the current state of the circuit without requiring async context.
    ///
    /// This is safe to call from sync code (e.g., metrics collection, health checks).
    pub fn state_sync(&self) -> CircuitState {
        CircuitState::from_u8(self.state_atomic.load(std::sync::atomic::Ordering::Acquire))
    }

    /// Returns an HTTP status code based on circuit state.
    ///
    /// - Closed: 200 (OK)
    /// - HalfOpen: 200 (OK) - accepting limited traffic
    /// - Open: 503 (Service Unavailable)
    pub fn http_status(&self) -> u16 {
        match self.state_sync() {
            CircuitState::Closed => 200,
            CircuitState::HalfOpen => 200,
            CircuitState::Open => 503,
        }
    }

    /// Returns a simple health status string.
    ///
    /// Returns "healthy" when circuit is closed, "degraded" when half-open,
    /// "unhealthy" when open.
    pub fn health_status(&self) -> &'static str {
        match self.state_sync() {
            CircuitState::Closed => "healthy",
            CircuitState::HalfOpen => "degraded",
            CircuitState::Open => "unhealthy",
        }
    }
}

impl<S, C> Clone for CircuitBreaker<S, C>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            circuit: Arc::clone(&self.circuit),
            state_atomic: Arc::clone(&self.state_atomic),
            config: Arc::clone(&self.config),
        }
    }
}

impl<S, C, Req> Service<Req> for CircuitBreaker<S, C>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
    Req: Send + 'static,
    C: FailureClassifier<S::Response, S::Error> + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = CircuitBreakerError<S::Error>;
    type Future = BoxFuture<'static, Result<S::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(CircuitBreakerError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let config = Arc::clone(&self.config);
        let circuit = Arc::clone(&self.circuit);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            #[cfg(feature = "tracing")]
            {
                let cb_name = &config.name;
                debug!(
                    breaker = cb_name,
                    "Checking if call is permitted by circuit breaker"
                );
            }

            let permitted = {
                let mut circuit = circuit.lock().await;
                circuit.try_acquire(&config)
            };

            #[cfg(feature = "tracing")]
            {
                let cb_name = &config.name;
                if permitted {
                    tracing::trace!(breaker = cb_name, "circuit breaker permitted call");
                } else {
                    tracing::trace!(
                        breaker = cb_name,
                        "circuit breaker rejected call (circuit open)"
                    );
                }
            }

            if !permitted {
                #[cfg(feature = "metrics")]
                {
                    counter!("circuitbreaker_calls_total", "circuitbreaker" => config.name.clone(), "outcome" => "rejected").increment(1);
                }

                return Err(CircuitBreakerError::OpenCircuit);
            }

            let start = std::time::Instant::now();
            let result = inner.call(req).await;
            let duration = start.elapsed();

            let mut circuit = circuit.lock().await;
            if config.failure_classifier.classify(&result) {
                circuit.record_failure(&config, duration);
            } else {
                circuit.record_success(&config, duration);
            }

            result.map_err(CircuitBreakerError::Inner)
        })
    }
}

/// A circuit breaker with a configured fallback handler.
///
/// This type is returned by [`CircuitBreaker::with_fallback`] and implements
/// `Service<Req>` with fallback behavior when the circuit is open.
pub struct CircuitBreakerWithFallback<S, C, Req, Res, Err> {
    inner: S,
    pub(crate) circuit: Arc<Mutex<Circuit>>,
    state_atomic: Arc<std::sync::atomic::AtomicU8>,
    pub(crate) config: Arc<CircuitBreakerConfig<C>>,
    fallback: SharedFallback<Req, Res, Err>,
    _phantom: std::marker::PhantomData<(Req, Res, Err)>,
}

impl<S, C, Req, Res, Err> CircuitBreakerWithFallback<S, C, Req, Res, Err> {
    /// Forces the circuit into the open state.
    pub async fn force_open(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_open(&self.config);
    }

    /// Forces the circuit into the closed state.
    pub async fn force_closed(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_closed(&self.config);
    }

    /// Returns whether the circuit is currently open.
    pub fn is_open(&self) -> bool {
        self.state_sync() == CircuitState::Open
    }

    /// Returns a snapshot of the current circuit breaker metrics.
    pub async fn metrics(&self) -> crate::circuit::CircuitMetrics {
        let circuit = self.circuit.lock().await;
        circuit.metrics(&self.config)
    }

    /// Resets the circuit to the closed state and clears counts.
    pub async fn reset(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.reset(&self.config);
    }

    /// Returns the current state of the circuit.
    pub async fn state(&self) -> CircuitState {
        let circuit = self.circuit.lock().await;
        circuit.state()
    }

    /// Returns the current state of the circuit without requiring async context.
    pub fn state_sync(&self) -> CircuitState {
        CircuitState::from_u8(self.state_atomic.load(std::sync::atomic::Ordering::Acquire))
    }

    /// Returns an HTTP status code based on circuit state.
    pub fn http_status(&self) -> u16 {
        match self.state_sync() {
            CircuitState::Closed => 200,
            CircuitState::HalfOpen => 200,
            CircuitState::Open => 503,
        }
    }

    /// Returns a simple health status string.
    pub fn health_status(&self) -> &'static str {
        match self.state_sync() {
            CircuitState::Closed => "healthy",
            CircuitState::HalfOpen => "degraded",
            CircuitState::Open => "unhealthy",
        }
    }
}

impl<S, C, Req, Res, Err> Clone for CircuitBreakerWithFallback<S, C, Req, Res, Err>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            circuit: Arc::clone(&self.circuit),
            state_atomic: Arc::clone(&self.state_atomic),
            config: Arc::clone(&self.config),
            fallback: Arc::clone(&self.fallback),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<S, C, Req, Res, Err> Service<Req> for CircuitBreakerWithFallback<S, C, Req, Res, Err>
where
    S: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
    Req: Send + 'static,
    C: FailureClassifier<Res, Err> + Send + Sync + 'static,
{
    type Response = Res;
    type Error = CircuitBreakerError<Err>;
    type Future = BoxFuture<'static, Result<Res, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(CircuitBreakerError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let config = Arc::clone(&self.config);
        let circuit = Arc::clone(&self.circuit);
        let mut inner = self.inner.clone();
        let fallback = Arc::clone(&self.fallback);

        Box::pin(async move {
            #[cfg(feature = "tracing")]
            {
                let cb_name = &config.name;
                debug!(
                    breaker = cb_name,
                    "Checking if call is permitted by circuit breaker"
                );
            }

            let permitted = {
                let mut circuit = circuit.lock().await;
                circuit.try_acquire(&config)
            };

            #[cfg(feature = "tracing")]
            {
                let cb_name = &config.name;
                if permitted {
                    tracing::trace!(breaker = cb_name, "circuit breaker permitted call");
                } else {
                    tracing::trace!(
                        breaker = cb_name,
                        "circuit breaker rejected call (circuit open)"
                    );
                }
            }

            if !permitted {
                #[cfg(feature = "metrics")]
                {
                    counter!("circuitbreaker_calls_total", "circuitbreaker" => config.name.clone(), "outcome" => "rejected").increment(1);
                }

                #[cfg(feature = "tracing")]
                {
                    let cb_name = &config.name;
                    tracing::debug!(breaker = cb_name, "Calling fallback handler");
                }

                return fallback(req).await.map_err(CircuitBreakerError::Inner);
            }

            let start = std::time::Instant::now();
            let result = inner.call(req).await;
            let duration = start.elapsed();

            let mut circuit = circuit.lock().await;
            if config.failure_classifier.classify(&result) {
                circuit.record_failure(&config, duration);
            } else {
                circuit.record_success(&config, duration);
            }

            result.map_err(CircuitBreakerError::Inner)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::DefaultClassifier;
    use std::time::Duration;

    fn dummy_config() -> CircuitBreakerConfig<DefaultClassifier> {
        use tower_resilience_core::EventListeners;
        CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: DefaultClassifier,
            minimum_number_of_calls: 10,
            slow_call_duration_threshold: None,
            slow_call_rate_threshold: 1.0,
            event_listeners: EventListeners::new(),
            name: "test".into(),
        }
    }

    #[test]
    fn transitions_to_open_on_high_failure_rate() {
        let mut circuit = Circuit::new();
        let config = dummy_config();

        for _ in 0..6 {
            circuit.record_failure(&config, Duration::from_millis(10));
        }
        for _ in 0..4 {
            circuit.record_success(&config, Duration::from_millis(10));
        }

        assert_eq!(circuit.state(), CircuitState::Open);
    }

    #[test]
    fn stays_closed_on_low_failure_rate() {
        let mut circuit = Circuit::new();
        let config = dummy_config();

        for _ in 0..2 {
            circuit.record_failure(&config, Duration::from_millis(10));
        }
        for _ in 0..8 {
            circuit.record_success(&config, Duration::from_millis(10));
        }

        assert_eq!(circuit.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn manual_override_controls_work() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        breaker.force_open().await;
        assert_eq!(breaker.state().await, CircuitState::Open);

        breaker.force_closed().await;
        assert_eq!(breaker.state().await, CircuitState::Closed);
    }

    #[test]
    fn test_error_helpers() {
        let err: CircuitBreakerError<&str> = CircuitBreakerError::OpenCircuit;
        assert!(err.is_circuit_open());
        assert_eq!(err.into_inner(), None);

        let err2 = CircuitBreakerError::Inner("fail");
        assert!(!err2.is_circuit_open());
        assert_eq!(err2.into_inner(), Some("fail"));
    }

    #[test]
    fn test_event_listeners() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower_resilience_core::EventListeners;

        let state_transitions = Arc::new(AtomicUsize::new(0));
        let call_permitted = Arc::new(AtomicUsize::new(0));
        let call_rejected = Arc::new(AtomicUsize::new(0));
        let successes = Arc::new(AtomicUsize::new(0));
        let failures = Arc::new(AtomicUsize::new(0));

        let st_clone = Arc::clone(&state_transitions);
        let cp_clone = Arc::clone(&call_permitted);
        let cr_clone = Arc::clone(&call_rejected);
        let s_clone = Arc::clone(&successes);
        let f_clone = Arc::clone(&failures);

        let config: CircuitBreakerConfig<DefaultClassifier> = CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: DefaultClassifier,
            minimum_number_of_calls: 10,
            slow_call_duration_threshold: None,
            slow_call_rate_threshold: 1.0,
            event_listeners: {
                let mut listeners = EventListeners::new();
                listeners.add(tower_resilience_core::FnListener::new(
                    move |event| match event {
                        CircuitBreakerEvent::StateTransition { .. } => {
                            st_clone.fetch_add(1, Ordering::SeqCst);
                        }
                        CircuitBreakerEvent::CallPermitted { .. } => {
                            cp_clone.fetch_add(1, Ordering::SeqCst);
                        }
                        CircuitBreakerEvent::CallRejected { .. } => {
                            cr_clone.fetch_add(1, Ordering::SeqCst);
                        }
                        CircuitBreakerEvent::SuccessRecorded { .. } => {
                            s_clone.fetch_add(1, Ordering::SeqCst);
                        }
                        CircuitBreakerEvent::FailureRecorded { .. } => {
                            f_clone.fetch_add(1, Ordering::SeqCst);
                        }
                        CircuitBreakerEvent::SlowCallDetected { .. } => {}
                    },
                ));
                listeners
            },
            name: "test".into(),
        };

        let mut circuit = Circuit::new();

        // Record failures to trigger state transition
        for _ in 0..6 {
            circuit.record_failure(&config, Duration::from_millis(10));
        }
        for _ in 0..4 {
            circuit.record_success(&config, Duration::from_millis(10));
        }

        // Should have transitioned to Open
        assert_eq!(circuit.state(), CircuitState::Open);
        assert_eq!(state_transitions.load(Ordering::SeqCst), 1);
        assert_eq!(failures.load(Ordering::SeqCst), 6);
        assert_eq!(successes.load(Ordering::SeqCst), 4);

        // Try acquiring (should be rejected)
        let permitted = circuit.try_acquire(&config);
        assert!(!permitted);
        assert_eq!(call_rejected.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_slow_call_detection() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower_resilience_core::EventListeners;

        let slow_calls = Arc::new(AtomicUsize::new(0));
        let slow_clone = Arc::clone(&slow_calls);

        let config: CircuitBreakerConfig<DefaultClassifier> = CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: DefaultClassifier,
            minimum_number_of_calls: 10,
            slow_call_duration_threshold: Some(Duration::from_millis(100)),
            slow_call_rate_threshold: 0.5,
            event_listeners: {
                let mut listeners = EventListeners::new();
                listeners.add(tower_resilience_core::FnListener::new(move |event| {
                    if matches!(event, CircuitBreakerEvent::SlowCallDetected { .. }) {
                        slow_clone.fetch_add(1, Ordering::SeqCst);
                    }
                }));
                listeners
            },
            name: "test".into(),
        };

        let mut circuit = Circuit::new();

        // Record 6 slow calls (>100ms)
        for _ in 0..6 {
            circuit.record_success(&config, Duration::from_millis(150));
        }
        // Record 4 fast calls
        for _ in 0..4 {
            circuit.record_success(&config, Duration::from_millis(50));
        }

        // Should have detected 6 slow calls
        assert_eq!(slow_calls.load(Ordering::SeqCst), 6);

        // Should have transitioned to Open due to slow call rate (60%)
        assert_eq!(circuit.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_sync_state() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> =
            CircuitBreaker::new((), config.clone());

        // Can access state synchronously without .await
        let sync_state = breaker.state_sync();
        assert_eq!(sync_state, CircuitState::Closed);

        // Force open and verify sync state matches
        breaker.force_open().await;
        assert_eq!(breaker.state_sync(), CircuitState::Open);
        assert_eq!(breaker.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_is_open_convenience_method() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        // Initially closed
        assert!(!breaker.is_open());
        assert_eq!(breaker.state_sync(), CircuitState::Closed);

        // Force open
        breaker.force_open().await;
        assert!(breaker.is_open());
        assert_eq!(breaker.state_sync(), CircuitState::Open);

        // Force closed
        breaker.force_closed().await;
        assert!(!breaker.is_open());
        assert_eq!(breaker.state_sync(), CircuitState::Closed);

        // Reset puts circuit back to closed
        breaker.force_open().await;
        assert!(breaker.is_open());
        breaker.reset().await;
        assert!(!breaker.is_open());
        assert_eq!(breaker.state_sync(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_metrics_snapshot() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        // Get initial metrics
        let metrics = breaker.metrics().await;
        assert_eq!(metrics.state, CircuitState::Closed);
        assert_eq!(metrics.total_calls, 0);
        assert_eq!(metrics.failure_count, 0);
        assert_eq!(metrics.success_count, 0);
        assert_eq!(metrics.failure_rate, 0.0);
        assert_eq!(metrics.slow_call_rate, 0.0);

        // Record some calls
        {
            let mut circuit = breaker.circuit.lock().await;
            circuit.record_success(&breaker.config, Duration::from_millis(10));
            circuit.record_success(&breaker.config, Duration::from_millis(10));
            circuit.record_failure(&breaker.config, Duration::from_millis(10));
        }

        // Get updated metrics
        let metrics = breaker.metrics().await;
        assert_eq!(metrics.total_calls, 3);
        assert_eq!(metrics.success_count, 2);
        assert_eq!(metrics.failure_count, 1);
        assert!((metrics.failure_rate - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_preset_standard() {
        let _layer = CircuitBreakerLayer::standard().build();
    }

    #[test]
    fn test_preset_fast_fail() {
        let _layer = CircuitBreakerLayer::fast_fail().build();
    }

    #[test]
    fn test_preset_tolerant() {
        let _layer = CircuitBreakerLayer::tolerant().build();
    }

    #[test]
    fn test_preset_with_customization() {
        // Verify presets can be further customized
        let _layer = CircuitBreakerLayer::standard()
            .name("my-service")
            .failure_rate_threshold(0.6)
            .build();
    }
}
