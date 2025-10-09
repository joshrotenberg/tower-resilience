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
//! ## Basic Example
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreaker};
//! use tower::service_fn;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder()
//!     .failure_rate_threshold(0.5)  // Open at 50% failure rate
//!     .sliding_window_size(100)     // Track last 100 calls
//!     .wait_duration_in_open(Duration::from_secs(30))
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//! let mut service: CircuitBreaker<_, String, String, ()> = layer.layer(svc);
//! # }
//! ```
//!
//! ## Time-Based Sliding Window
//!
//! Use time-based windows instead of count-based:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreaker, SlidingWindowType};
//! use tower::service_fn;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_type(SlidingWindowType::TimeBased)
//!     .sliding_window_duration(Duration::from_secs(60))  // Last 60 seconds
//!     .minimum_number_of_calls(10)
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//! let mut service: CircuitBreaker<_, String, String, ()> = layer.layer(svc);
//! # }
//! ```
//!
//! ## Fallback Handler
//!
//! Provide fallback responses when circuit is open:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::service_fn;
//! use std::time::Duration;
//! use futures::future::BoxFuture;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .build();
//!
//! let base_service = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//!
//! let mut service = layer.layer(base_service)
//!     .with_fallback(|_req: String| -> BoxFuture<'static, Result<String, ()>> {
//!         Box::pin(async {
//!             Ok("fallback response".to_string())
//!         })
//!     });
//! # }
//! ```
//!
//! ## Custom Failure Classification
//!
//! Control what counts as a failure:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreaker};
//! use tower::service_fn;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, std::io::Error>::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .failure_classifier(|result: &Result<String, std::io::Error>| {
//!         match result {
//!             // Don't count timeouts as failures
//!             Err(e) if e.kind() == std::io::ErrorKind::TimedOut => false,
//!             Err(_) => true,
//!             Ok(_) => false,
//!         }
//!     })
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, std::io::Error>(req)
//! });
//! let mut service: CircuitBreaker<_, String, String, std::io::Error> = layer.layer(svc);
//! # }
//! ```
//!
//! ## Slow Call Detection
//!
//! Open circuit based on slow calls:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreaker};
//! use tower::service_fn;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder()
//!     .failure_rate_threshold(1.0)  // Don't open on failures
//!     .slow_call_duration_threshold(Duration::from_secs(2))
//!     .slow_call_rate_threshold(0.5)  // Open at 50% slow calls
//!     .sliding_window_size(100)
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//! let mut service: CircuitBreaker<_, String, String, ()> = layer.layer(svc);
//! # }
//! ```
//!
//! ## Event Listeners
//!
//! Monitor circuit breaker behavior:
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreaker};
//! use tower::service_fn;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder()
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
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//! let mut service: CircuitBreaker<_, String, String, ()> = layer.layer(svc);
//! # }
//! ```
//!
//! ## Error Handling
//!
//! ```rust
//! use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitBreakerError};
//! use tower::{Service, service_fn};
//!
//! # async fn example() {
//! let layer = CircuitBreakerLayer::<String, ()>::builder().build();
//! let mut service = layer.layer(service_fn(|req: String| async move {
//!     Ok::<_, ()>(req)
//! }));
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
//! ## Features
//! - Count-based and time-based sliding windows
//! - Configurable failure rate threshold
//! - Slow call detection and rate threshold
//! - Half-open state for gradual recovery
//! - Event system for observability
//! - Optional fallback handling
//! - Manual state control (force_open, force_closed, reset)
//! - Sync state inspection with `state_sync()`
//! - Metrics integration via `metrics` feature
//! - Tracing support via `tracing` feature
//!
//! ## Feature Flags
//! - `metrics`: enables metrics collection using the `metrics` crate
//! - `tracing`: enables logging and tracing using the `tracing` crate

use crate::circuit::Circuit;
use futures::future::BoxFuture;
#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter, describe_gauge};
use std::sync::Arc;
#[cfg(feature = "metrics")]
use std::sync::Once;
use std::task::{Context, Poll};
use tokio::sync::Mutex;
use tower::Service;
#[cfg(feature = "tracing")]
use tracing::debug;

pub use circuit::CircuitState;
pub use config::{CircuitBreakerConfig, CircuitBreakerConfigBuilder, SlidingWindowType};
pub use error::CircuitBreakerError;
pub use events::CircuitBreakerEvent;
pub use layer::CircuitBreakerLayer;

mod circuit;
mod config;
mod error;
mod events;
mod layer;

pub(crate) type FailureClassifier<Res, Err> = dyn Fn(&Result<Res, Err>) -> bool + Send + Sync;
pub(crate) type SharedFailureClassifier<Res, Err> = Arc<FailureClassifier<Res, Err>>;

pub(crate) type FallbackFn<Req, Res, Err> =
    dyn Fn(Req) -> BoxFuture<'static, Result<Res, Err>> + Send + Sync;
pub(crate) type SharedFallback<Req, Res, Err> = Arc<FallbackFn<Req, Res, Err>>;

#[cfg(feature = "metrics")]
static METRICS_INIT: Once = Once::new();

/// Returns a new builder for a `CircuitBreakerLayer`.
///
/// This is a convenience function that returns a builder. You can also use
/// `CircuitBreakerLayer::builder()` directly.
pub fn circuit_breaker_builder<Res, Err>() -> CircuitBreakerConfigBuilder<Res, Err> {
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
            describe_gauge!(
                "circuitbreaker_state",
                "Current state of the circuit breaker"
            );
        });
    }
    CircuitBreakerConfigBuilder::default()
}

/// A Tower Service that applies circuit breaker logic to an inner service.
///
/// Manages the circuit state and controls calls to the inner service accordingly.
pub struct CircuitBreaker<S, Req, Res, Err> {
    inner: S,
    circuit: Arc<Mutex<Circuit>>,
    state_atomic: Arc<std::sync::atomic::AtomicU8>,
    config: Arc<CircuitBreakerConfig<Res, Err>>,
    fallback: Option<SharedFallback<Req, Res, Err>>,
    _phantom: std::marker::PhantomData<Req>,
}

impl<S, Req, Res, Err> CircuitBreaker<S, Req, Res, Err> {
    /// Creates a new `CircuitBreaker` wrapping the given service and configuration.
    pub(crate) fn new(inner: S, config: Arc<CircuitBreakerConfig<Res, Err>>) -> Self {
        let state_atomic = Arc::new(std::sync::atomic::AtomicU8::new(CircuitState::Closed as u8));
        Self {
            inner,
            circuit: Arc::new(Mutex::new(Circuit::new_with_atomic(Arc::clone(
                &state_atomic,
            )))),
            state_atomic,
            config,
            fallback: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets a fallback function to be called when the circuit is open.
    pub fn with_fallback<F>(mut self, fallback: F) -> Self
    where
        F: Fn(Req) -> BoxFuture<'static, Result<Res, Err>> + Send + Sync + 'static,
    {
        self.fallback = Some(Arc::new(fallback));
        self
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
    /// Reads from an AtomicU8 that's kept synchronized with the actual state.
    pub fn state_sync(&self) -> CircuitState {
        CircuitState::from_u8(self.state_atomic.load(std::sync::atomic::Ordering::Acquire))
    }
}

impl<S, Req, Res, Err> Service<Req> for CircuitBreaker<S, Req, Res, Err>
where
    S: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
    Req: Send + 'static,
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
        let fallback = self.fallback.clone();

        Box::pin(async move {
            #[cfg(feature = "tracing")]
            {
                let cb_name = &config.name;
                debug!(
                    breaker = cb_name,
                    "Checking if call is permitted by circuit breaker"
                );
            }

            #[cfg(feature = "tracing")]
            let circuit_check_span = {
                use tracing::{span, Level};
                let state = {
                    // To avoid holding the lock too long, just get the state for span field.
                    let circuit = circuit.lock().await;
                    circuit.state()
                };
                let cb_name = &config.name;
                span!(Level::DEBUG, "circuit_check", breaker = cb_name, state = ?state)
            };
            #[cfg(feature = "tracing")]
            let _enter = circuit_check_span.enter();

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
                    let counter = counter!("circuitbreaker_calls_total", "outcome" => "rejected");
                    counter.increment(1);
                }

                // If a fallback is configured, call it instead of returning an error
                if let Some(fallback_fn) = fallback {
                    #[cfg(feature = "tracing")]
                    {
                        let cb_name = &config.name;
                        tracing::debug!(breaker = cb_name, "Calling fallback handler");
                    }

                    return fallback_fn(req).await.map_err(CircuitBreakerError::Inner);
                }

                return Err(CircuitBreakerError::OpenCircuit);
            }

            let start = std::time::Instant::now();
            let result = inner.call(req).await;
            let duration = start.elapsed();

            let mut circuit = circuit.lock().await;
            if (config.failure_classifier)(&result) {
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
    use std::time::Duration;

    fn dummy_config() -> CircuitBreakerConfig<(), ()> {
        use tower_resilience_core::EventListeners;
        CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
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
        let breaker: CircuitBreaker<(), (), (), ()> = CircuitBreaker::new((), config);

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

        let config: CircuitBreakerConfig<(), ()> = CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
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

        let config: CircuitBreakerConfig<(), ()> = CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
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

    #[test]
    fn test_slow_call_with_failures() {
        use tower_resilience_core::EventListeners;

        let config: CircuitBreakerConfig<(), ()> = CircuitBreakerConfig {
            failure_rate_threshold: 1.0, // Don't open on failures
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
            minimum_number_of_calls: 10,
            slow_call_duration_threshold: Some(Duration::from_millis(100)),
            slow_call_rate_threshold: 0.5,
            event_listeners: EventListeners::new(),
            name: "test".into(),
        };

        let mut circuit = Circuit::new();

        // Record 6 slow failures (failures can also be slow)
        for _ in 0..6 {
            circuit.record_failure(&config, Duration::from_millis(150));
        }
        // Record 4 fast successes
        for _ in 0..4 {
            circuit.record_success(&config, Duration::from_millis(50));
        }

        // Should open due to slow call rate, not failure rate
        assert_eq!(circuit.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_sync_state() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), (), (), ()> = CircuitBreaker::new((), config.clone());

        // Can access state synchronously without .await
        let sync_state = breaker.state_sync();
        assert_eq!(sync_state, CircuitState::Closed);

        // Force open and verify sync state matches
        breaker.force_open().await;
        assert_eq!(breaker.state_sync(), CircuitState::Open);
        assert_eq!(breaker.state().await, CircuitState::Open);
    }
}
