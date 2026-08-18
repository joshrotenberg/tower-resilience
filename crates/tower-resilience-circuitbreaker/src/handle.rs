use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::circuit::{Circuit, CircuitMetrics, CircuitState};
use crate::config::CircuitBreakerConfig;

/// A deterministic shared handle for observing and controlling circuit
/// breaker state from outside the service.
///
/// Obtained from [`crate::CircuitBreakerConfigBuilder::build_with_handle()`]. The handle
/// is cheap to clone and safe to share across threads (`Clone + Send + Sync`).
///
/// This is useful when the circuit breaker service is consumed by middleware
/// (e.g., wrapped in `BoxCloneService`) and direct access to state inspection
/// or control methods on [`CircuitBreaker`](crate::CircuitBreaker) is no
/// longer available: the handle holds its own `Arc` to the circuit shared by
/// every service the associated layer produces, so it keeps working after
/// those services are moved, boxed, or dropped.
///
/// In addition to the read-only inspection methods below,
/// [`Self::force_open`], [`Self::force_closed`], and [`Self::reset`] give a
/// deterministic, awaitable control API: once a call to one of them
/// completes, every subsequent state inspection or admission check across
/// every clone of every service produced by the layer observes the new
/// state, with no sleep or poll loop required. Combine with
/// [`CircuitBreakerConfigBuilder::manual_mode()`](crate::CircuitBreakerConfigBuilder::manual_mode)
/// for a circuit that changes state only in response to these explicit
/// calls -- a simple external on/off switch.
///
/// # Example
///
/// ```rust
/// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
///
/// # async fn example() {
/// let (layer, handle) = CircuitBreakerLayer::builder()
///     .failure_rate_threshold(0.5)
///     .build_with_handle()
///     .unwrap();
///
/// // Apply the layer to a service (consumes into BoxCloneService, etc.)
/// // ...
///
/// // Later, query state from the handle:
/// let state = handle.state();
/// let health = handle.health_status();
/// assert_eq!(health, "healthy");
///
/// // Or take deterministic external control, even though the original
/// // service may have since been moved or boxed:
/// handle.force_open().await;
/// assert!(handle.is_open());
/// # }
/// ```
#[derive(Clone)]
pub struct CircuitBreakerHandle<C = crate::classifier::DefaultClassifier> {
    pub(crate) circuit: Arc<Mutex<Circuit>>,
    pub(crate) state_atomic: Arc<std::sync::atomic::AtomicU8>,
    pub(crate) config: Arc<CircuitBreakerConfig<C>>,
}

impl<C> CircuitBreakerHandle<C> {
    /// Returns the current state of the circuit without requiring async context.
    ///
    /// Uses an atomic load (Acquire ordering) for lock-free access.
    pub fn state(&self) -> CircuitState {
        CircuitState::from_u8(self.state_atomic.load(Ordering::Acquire))
    }

    /// Returns whether the circuit is currently open.
    pub fn is_open(&self) -> bool {
        self.state() == CircuitState::Open
    }

    /// Returns a simple health status string.
    ///
    /// - `"healthy"` when circuit is closed
    /// - `"degraded"` when half-open
    /// - `"unhealthy"` when open
    pub fn health_status(&self) -> &'static str {
        match self.state() {
            CircuitState::Closed => "healthy",
            CircuitState::HalfOpen => "degraded",
            CircuitState::Open => "unhealthy",
        }
    }

    /// Returns an HTTP status code based on circuit state.
    ///
    /// - Closed: 200 (OK)
    /// - HalfOpen: 200 (OK) - accepting limited traffic
    /// - Open: 503 (Service Unavailable)
    pub fn http_status(&self) -> u16 {
        match self.state() {
            CircuitState::Closed | CircuitState::HalfOpen => 200,
            CircuitState::Open => 503,
        }
    }

    /// Returns a snapshot of the current circuit breaker metrics.
    ///
    /// Requires an async context because it locks the internal circuit state.
    pub async fn metrics(&self) -> CircuitMetrics {
        let circuit = self.circuit.lock().await;
        circuit.metrics(&self.config)
    }

    /// Forces the circuit into the open state.
    ///
    /// This is deterministic: the underlying state is updated (with
    /// `Release` ordering) before this call returns, so any subsequent
    /// `state()`/`is_open()`/`health_status()` call (which loads with
    /// `Acquire` ordering) is guaranteed to observe `Open` -- no sleep or
    /// poll loop is needed. Because the handle holds its own `Arc` to the
    /// circuit shared by [`crate::CircuitBreakerConfigBuilder::build_with_handle()`],
    /// every service produced by that layer -- including clones made before
    /// or after this call, and services already moved or boxed into another
    /// stack -- observes the new state on their very next admission check.
    ///
    /// Emits the same [`crate::CircuitBreakerEvent::StateTransition`] event
    /// and metrics as an automatic trip.
    ///
    /// # Example
    /// ```rust
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// # async fn example() {
    /// let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();
    /// handle.force_open().await;
    /// assert!(handle.is_open());
    /// # }
    /// ```
    pub async fn force_open(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_open(&self.config);
    }

    /// Forces the circuit into the closed state.
    ///
    /// See [`Self::force_open`] for the determinism, cross-clone, and
    /// observability guarantees, which apply equally here.
    ///
    /// # Example
    /// ```rust
    /// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    ///
    /// # async fn example() {
    /// let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();
    /// handle.force_open().await;
    /// handle.force_closed().await;
    /// assert!(!handle.is_open());
    /// # }
    /// ```
    pub async fn force_closed(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.force_closed(&self.config);
    }

    /// Resets the circuit to the closed state and clears counts.
    ///
    /// See [`Self::force_open`] for the determinism, cross-clone, and
    /// observability guarantees, which apply equally here.
    pub async fn reset(&self) {
        let mut circuit = self.circuit.lock().await;
        circuit.reset(&self.config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CircuitBreakerLayer;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::Poll;
    use tower::{Layer, Service, ServiceExt};

    #[derive(Clone)]
    struct OkService;

    impl Service<String> for OkService {
        type Response = String;
        type Error = String;
        type Future = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: String) -> Self::Future {
            Box::pin(async move { Ok(req) })
        }
    }

    #[derive(Clone)]
    struct ErrService;

    impl Service<String> for ErrService {
        type Response = String;
        type Error = String;
        type Future = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: String) -> Self::Future {
            Box::pin(async move { Err("fail".to_string()) })
        }
    }

    #[tokio::test]
    async fn test_handle_initial_state() {
        let (_layer, handle) = CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .build_with_handle()
            .unwrap();

        assert_eq!(handle.state(), CircuitState::Closed);
        assert_eq!(handle.health_status(), "healthy");
        assert!(!handle.is_open());
        assert_eq!(handle.http_status(), 200);
    }

    #[tokio::test]
    async fn test_handle_observes_state_changes() {
        let (layer, handle) = CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(4)
            .minimum_number_of_calls(4)
            .build_with_handle()
            .unwrap();

        let mut svc = layer.layer(ErrService);

        // Send enough failures to trip the breaker (50% threshold, 4 call window)
        // Need all 4 to fail so failure rate = 100% > 50%
        for _ in 0..4 {
            let _ = svc.call("test".to_string()).await;
        }

        assert_eq!(handle.state(), CircuitState::Open);
        assert_eq!(handle.health_status(), "unhealthy");
        assert!(handle.is_open());
        assert_eq!(handle.http_status(), 503);
    }

    #[tokio::test]
    async fn test_handle_metrics() {
        let (layer, handle) = CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(10)
            .build_with_handle()
            .unwrap();

        let svc = layer.layer(OkService);
        let _ = svc.oneshot("test".to_string()).await;

        let metrics = handle.metrics().await;
        assert_eq!(metrics.state, CircuitState::Closed);
        assert_eq!(metrics.total_calls, 1);
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.failure_count, 0);
    }

    #[tokio::test]
    async fn test_handle_shared_across_cloned_services() {
        let (layer, handle) = CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(4)
            .minimum_number_of_calls(4)
            .build_with_handle()
            .unwrap();

        // Create two services from the same layer -- they share state
        let mut svc1 = layer.layer(ErrService);
        let mut svc2 = layer.layer(ErrService);

        // Failures across both services accumulate in the same circuit
        let _ = svc1.call("a".to_string()).await;
        let _ = svc2.call("b".to_string()).await;
        let _ = svc1.call("c".to_string()).await;
        let _ = svc2.call("d".to_string()).await;

        // The shared circuit should be open now -- failures from both
        // services accumulated in the same circuit
        assert_eq!(handle.state(), CircuitState::Open);
        assert!(handle.is_open());
    }

    #[tokio::test]
    async fn test_handle_clone_is_independent() {
        let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();

        let handle2 = handle.clone();

        // Both observe the same state
        assert_eq!(handle.state(), handle2.state());
        assert_eq!(handle.health_status(), handle2.health_status());
    }

    #[tokio::test]
    async fn test_handle_force_open_is_immediately_observable() {
        let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();

        assert_eq!(handle.state(), CircuitState::Closed);

        handle.force_open().await;

        // No sleep or poll loop -- the state is guaranteed visible as soon
        // as `force_open().await` returns.
        assert_eq!(handle.state(), CircuitState::Open);
        assert!(handle.is_open());
        assert_eq!(handle.health_status(), "unhealthy");
        assert_eq!(handle.http_status(), 503);
    }

    #[tokio::test]
    async fn test_handle_force_closed_is_immediately_observable() {
        let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();

        handle.force_open().await;
        assert_eq!(handle.state(), CircuitState::Open);

        handle.force_closed().await;
        assert_eq!(handle.state(), CircuitState::Closed);
        assert!(!handle.is_open());
    }

    #[tokio::test]
    async fn test_handle_reset_clears_counts_and_closes() {
        let (layer, handle) = CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(4)
            .minimum_number_of_calls(4)
            .build_with_handle()
            .unwrap();

        let mut svc = layer.layer(ErrService);
        for _ in 0..4 {
            let _ = svc.call("x".to_string()).await;
        }
        assert_eq!(handle.state(), CircuitState::Open);

        handle.reset().await;

        assert_eq!(handle.state(), CircuitState::Closed);
        let metrics = handle.metrics().await;
        assert_eq!(metrics.total_calls, 0);
        assert_eq!(metrics.failure_count, 0);
    }

    #[tokio::test]
    async fn test_handle_force_open_rejects_calls_on_every_clone() {
        let (layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();

        // Two independently created services from the same layer.
        let mut svc1 = layer.layer(OkService);
        let mut svc2 = layer.layer(OkService);

        handle.force_open().await;

        // Every service produced by the layer -- including ones created
        // before this call -- observes the new state on its very next
        // admission check, with no sleep required.
        let err1 = svc1.call("a".to_string()).await.unwrap_err();
        let err2 = svc2.call("b".to_string()).await.unwrap_err();
        assert!(matches!(err1, crate::CircuitBreakerError::OpenCircuit));
        assert!(matches!(err2, crate::CircuitBreakerError::OpenCircuit));
    }

    #[tokio::test]
    async fn test_handle_controls_boxed_and_moved_service() {
        use tower::util::BoxCloneService;

        let (layer, handle) = CircuitBreakerLayer::builder().build_with_handle().unwrap();

        // Apply the layer, then erase the concrete type and move it away --
        // the only remaining way to reach the circuit is through `handle`.
        let svc = layer.layer(OkService);
        let mut boxed: BoxCloneService<String, String, crate::CircuitBreakerError<String>> =
            BoxCloneService::new(svc);
        drop(layer);

        handle.force_open().await;

        let err = boxed.call("x".to_string()).await.unwrap_err();
        assert!(matches!(err, crate::CircuitBreakerError::OpenCircuit));

        handle.force_closed().await;
        let ok = boxed.call("x".to_string()).await.unwrap();
        assert_eq!(ok, "x");
    }

    #[tokio::test]
    async fn test_handle_force_open_emits_same_state_transition_event_as_automatic_trip() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let transitions = Arc::new(AtomicUsize::new(0));
        let observed_to_open = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let t_clone = Arc::clone(&transitions);
        let o_clone = Arc::clone(&observed_to_open);

        let (_layer, handle) = CircuitBreakerLayer::builder()
            .on_state_transition(move |from, to| {
                t_clone.fetch_add(1, Ordering::SeqCst);
                if from == CircuitState::Closed && to == CircuitState::Open {
                    o_clone.store(true, Ordering::SeqCst);
                }
            })
            .build_with_handle()
            .unwrap();

        handle.force_open().await;

        // Externally initiated transitions emit the exact same
        // StateTransition event an automatic trip would.
        assert_eq!(transitions.load(Ordering::SeqCst), 1);
        assert!(observed_to_open.load(Ordering::SeqCst));
    }
}
