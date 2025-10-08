//!
//! A Tower middleware implementing circuit breaker behavior to improve the resilience of asynchronous services.
//!
//! ## Features
//! - Circuit breaker states: Closed, Open, Half-Open
//! - Configurable failure rate threshold and sliding window size
//! - Customizable `failure_classifier` to define what counts as a failure
//! - Metrics support via the `metrics` feature flag
//! - Tracing support via the `tracing` feature flag
//!
//! ## Example
//! ```rust
//! use tower_circuitbreaker::CircuitBreakerConfig;
//! use tower::service_fn;
//! use tower::Service;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Build a circuit breaker layer with custom settings
//!     let circuit_breaker_layer = CircuitBreakerConfig::builder()
//!         .failure_rate_threshold(0.3)
//!         .sliding_window_size(50)
//!         .wait_duration_in_open(Duration::from_secs(10))
//!         .build();
//!
//!     // Create a minimal service that echoes the request
//!     let my_service = service_fn(|req| async move { Ok::<_, ()>(req) });
//!
//!     // Wrap the service with the circuit breaker
//!     let mut service = circuit_breaker_layer.layer(my_service);
//!
//!     // Use the service
//!     let response = Service::call(&mut service, "hello").await.unwrap();
//!     assert_eq!(response, "hello");
//!
//!     // Error handling example
//!     match Service::call(&mut service, "hello").await {
//!         Ok(resp) => println!("got {}", resp),
//!         Err(e) if e.is_circuit_open() => println!("circuit open"),
//!         Err(e) => eprintln!("service error: {:?}", e.into_inner()),
//!     }
//! }
//! ```
//!
//! ## Feature Flags
//! - `metrics`: enables metrics collection using the `metrics` crate.
//! - `tracing`: enables logging and tracing using the `tracing` crate.

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
pub use config::{CircuitBreakerConfig, CircuitBreakerConfigBuilder};
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
/// `CircuitBreakerConfig::builder()` directly.
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
    config: Arc<CircuitBreakerConfig<Res, Err>>,
    fallback: Option<SharedFallback<Req, Res, Err>>,
    _phantom: std::marker::PhantomData<Req>,
}

impl<S, Req, Res, Err> CircuitBreaker<S, Req, Res, Err> {
    /// Creates a new `CircuitBreaker` wrapping the given service and configuration.
    pub(crate) fn new(inner: S, config: Arc<CircuitBreakerConfig<Res, Err>>) -> Self {
        Self {
            inner,
            circuit: Arc::new(Mutex::new(Circuit::new())),
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
                use tracing::{Level, span};
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

            let result = inner.call(req).await;

            let mut circuit = circuit.lock().await;
            if (config.failure_classifier)(&result) {
                circuit.record_failure(&config);
            } else {
                circuit.record_success(&config);
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
            sliding_window_size: 10,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
            minimum_number_of_calls: 10,
            event_listeners: EventListeners::new(),
            name: "test".into(),
        }
    }

    #[test]
    fn transitions_to_open_on_high_failure_rate() {
        let mut circuit = Circuit::new();
        let config = dummy_config();

        for _ in 0..6 {
            circuit.record_failure(&config);
        }
        for _ in 0..4 {
            circuit.record_success(&config);
        }

        assert_eq!(circuit.state(), CircuitState::Open);
    }

    #[test]
    fn stays_closed_on_low_failure_rate() {
        let mut circuit = Circuit::new();
        let config = dummy_config();

        for _ in 0..2 {
            circuit.record_failure(&config);
        }
        for _ in 0..8 {
            circuit.record_success(&config);
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
            sliding_window_size: 10,
            wait_duration_in_open: Duration::from_secs(1),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|r| r.is_err()),
            minimum_number_of_calls: 10,
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
                    },
                ));
                listeners
            },
            name: "test".into(),
        };

        let mut circuit = Circuit::new();

        // Record failures to trigger state transition
        for _ in 0..6 {
            circuit.record_failure(&config);
        }
        for _ in 0..4 {
            circuit.record_success(&config);
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
}
