use crate::config::RateLimiterConfig;
use crate::limiter::SharedRateLimiter;
use std::sync::Arc;

/// A read-only handle for observing rate limiter state.
///
/// Obtained from [`crate::RateLimiterConfigBuilder::build_with_handle()`]. The handle
/// is cheap to clone and safe to share across threads (`Clone + Send + Sync`).
///
/// This is useful when the rate limiter service is consumed by middleware
/// (e.g., wrapped in `BoxCloneService`) and direct access to the service
/// is no longer available.
///
/// # Example
///
/// ```rust
/// use tower_resilience_ratelimiter::RateLimiterLayer;
///
/// let (layer, handle) = RateLimiterLayer::builder()
///     .limit_for_period(100)
///     .build_with_handle();
///
/// // Apply the layer to a service...
///
/// // Later, query state from the handle:
/// let available = handle.available_permits();
/// ```
#[derive(Clone)]
pub struct RateLimiterHandle {
    pub(crate) limiter: SharedRateLimiter,
    pub(crate) config: Arc<RateLimiterConfig>,
}

impl RateLimiterHandle {
    /// Returns the number of permits currently available.
    pub fn available_permits(&self) -> usize {
        self.limiter.available_permits()
    }

    /// Returns whether the rate limiter has no permits available.
    pub fn is_throttling(&self) -> bool {
        self.available_permits() == 0
    }

    /// Returns the utilization ratio (0.0 to 1.0).
    ///
    /// A value of 1.0 means all permits are consumed.
    pub fn utilization(&self) -> f64 {
        let limit = self.config.limit_for_period;
        if limit == 0 {
            return 0.0;
        }
        let available = self.available_permits();
        let used = limit.saturating_sub(available);
        used as f64 / limit as f64
    }

    /// Returns the configured limit per period.
    pub fn limit_for_period(&self) -> usize {
        self.config.limit_for_period
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiterLayer;
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

    #[tokio::test]
    async fn test_handle_initial_state() {
        let (_layer, handle) = RateLimiterLayer::builder()
            .limit_for_period(10)
            .build_with_handle();

        assert_eq!(handle.available_permits(), 10);
        assert!(!handle.is_throttling());
        assert_eq!(handle.utilization(), 0.0);
        assert_eq!(handle.limit_for_period(), 10);
    }

    #[tokio::test]
    async fn test_handle_observes_permit_usage() {
        let (layer, handle) = RateLimiterLayer::builder()
            .limit_for_period(3)
            .build_with_handle();

        let mut svc = layer.layer(OkService);

        // Use one permit
        let _ = svc.call("a".to_string()).await;
        assert_eq!(handle.available_permits(), 2);

        // Use another
        let _ = svc.call("b".to_string()).await;
        assert_eq!(handle.available_permits(), 1);
    }

    #[tokio::test]
    async fn test_handle_shared_across_services() {
        let (layer, handle) = RateLimiterLayer::builder()
            .limit_for_period(4)
            .build_with_handle();

        let mut svc1 = layer.layer(OkService);
        let mut svc2 = layer.layer(OkService);

        let _ = svc1.call("a".to_string()).await;
        let _ = svc2.call("b".to_string()).await;

        // Both services consumed from the same pool
        assert_eq!(handle.available_permits(), 2);
    }

    #[tokio::test]
    async fn test_handle_clone() {
        let (_layer, handle) = RateLimiterLayer::builder()
            .limit_for_period(10)
            .build_with_handle();

        let handle2 = handle.clone();
        assert_eq!(handle.available_permits(), handle2.available_permits());
    }
}
