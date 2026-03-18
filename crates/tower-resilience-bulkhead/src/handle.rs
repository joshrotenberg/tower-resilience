use crate::config::BulkheadConfig;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// A read-only handle for observing bulkhead state.
///
/// Obtained from [`crate::BulkheadConfigBuilder::build_with_handle()`]. The handle
/// is cheap to clone and safe to share across threads (`Clone + Send + Sync`).
///
/// This is useful when the bulkhead service is consumed by middleware
/// (e.g., wrapped in `BoxCloneService`) and direct access to the service
/// is no longer available.
///
/// # Example
///
/// ```rust
/// use tower_resilience_bulkhead::BulkheadLayer;
///
/// let (layer, handle) = BulkheadLayer::builder()
///     .max_concurrent_calls(10)
///     .build_with_handle();
///
/// // Apply the layer to a service...
///
/// // Later, query state from the handle:
/// assert_eq!(handle.active_calls(), 0);
/// assert_eq!(handle.max_concurrent(), 10);
/// ```
#[derive(Clone)]
pub struct BulkheadHandle {
    pub(crate) semaphore: Arc<Semaphore>,
    pub(crate) config: Arc<BulkheadConfig>,
}

impl BulkheadHandle {
    /// Returns the number of currently active (in-flight) calls.
    pub fn active_calls(&self) -> usize {
        self.config
            .max_concurrent_calls
            .saturating_sub(self.semaphore.available_permits())
    }

    /// Returns the configured maximum concurrent calls.
    pub fn max_concurrent(&self) -> usize {
        self.config.max_concurrent_calls
    }

    /// Returns the utilization ratio (0.0 to 1.0).
    ///
    /// A value of 1.0 means all permits are consumed.
    pub fn utilization(&self) -> f64 {
        let max = self.config.max_concurrent_calls;
        if max == 0 {
            return 0.0;
        }
        self.active_calls() as f64 / max as f64
    }

    /// Returns the number of available permits.
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BulkheadLayer;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::Poll;
    use tower::{Layer, Service};

    #[derive(Clone)]
    struct SlowService;

    impl Service<String> for SlowService {
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
            Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                Ok(req)
            })
        }
    }

    #[tokio::test]
    async fn test_handle_initial_state() {
        let (_layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(10)
            .build_with_handle();

        assert_eq!(handle.active_calls(), 0);
        assert_eq!(handle.max_concurrent(), 10);
        assert_eq!(handle.available_permits(), 10);
        assert_eq!(handle.utilization(), 0.0);
    }

    #[tokio::test]
    async fn test_handle_observes_active_calls() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(5)
            .build_with_handle();

        let mut svc = layer.layer(SlowService);

        // Spawn the call as a task so it actually runs
        let task = tokio::spawn(async move { svc.call("test".to_string()).await });

        // Give it a moment to acquire the permit
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(handle.active_calls(), 1);
        assert_eq!(handle.available_permits(), 4);

        // Wait for it to complete
        let _ = task.await;
        assert_eq!(handle.active_calls(), 0);
    }

    #[tokio::test]
    async fn test_handle_shared_across_services() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(10)
            .build_with_handle();

        let mut svc1 = layer.layer(SlowService);
        let mut svc2 = layer.layer(SlowService);

        // Spawn as tasks so they actually run concurrently
        let t1 = tokio::spawn(async move { svc1.call("a".to_string()).await });
        let t2 = tokio::spawn(async move { svc2.call("b".to_string()).await });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(handle.active_calls(), 2);
        assert_eq!(handle.available_permits(), 8);

        let _ = tokio::join!(t1, t2);
        assert_eq!(handle.active_calls(), 0);
    }

    #[tokio::test]
    async fn test_handle_clone() {
        let (_layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(10)
            .build_with_handle();

        let handle2 = handle.clone();
        assert_eq!(handle.active_calls(), handle2.active_calls());
        assert_eq!(handle.max_concurrent(), handle2.max_concurrent());
    }
}
