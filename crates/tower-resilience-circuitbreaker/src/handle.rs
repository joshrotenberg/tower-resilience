use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::circuit::{Circuit, CircuitMetrics, CircuitState};
use crate::config::CircuitBreakerConfig;

/// A read-only handle for observing circuit breaker state.
///
/// Obtained from [`CircuitBreakerConfigBuilder::build_with_handle()`]. The handle
/// is cheap to clone and safe to share across threads (`Clone + Send + Sync`).
///
/// This is useful when the circuit breaker service is consumed by middleware
/// (e.g., wrapped in `BoxCloneService`) and direct access to state inspection
/// methods on [`CircuitBreaker`](crate::CircuitBreaker) is no longer available.
///
/// # Example
///
/// ```rust
/// use tower_resilience_circuitbreaker::CircuitBreakerLayer;
///
/// let (layer, handle) = CircuitBreakerLayer::builder()
///     .failure_rate_threshold(0.5)
///     .build_with_handle();
///
/// // Apply the layer to a service (consumes into BoxCloneService, etc.)
/// // ...
///
/// // Later, query state from the handle:
/// let state = handle.state();
/// let health = handle.health_status();
/// assert_eq!(health, "healthy");
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
            .build_with_handle();

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
            .build_with_handle();

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
            .build_with_handle();

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
            .build_with_handle();

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
        let (_layer, handle) = CircuitBreakerLayer::builder().build_with_handle();

        let handle2 = handle.clone();

        // Both observe the same state
        assert_eq!(handle.state(), handle2.state());
        assert_eq!(handle.health_status(), handle2.health_status());
    }
}
