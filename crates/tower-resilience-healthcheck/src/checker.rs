//! Health checker trait and implementations.

use crate::HealthStatus;
use std::future::Future;

/// Trait for checking the health of a resource.
///
/// Implementors define how to check if a resource (Redis connection, HTTP client, etc.)
/// is healthy.
///
/// # Examples
///
/// Using a closure (via blanket impl):
///
/// ```rust
/// use tower_resilience_healthcheck::{HealthChecker, HealthStatus};
///
/// let redis_checker = |conn: &String| async move {
///     // Your health check logic
///     HealthStatus::Healthy
/// };
/// ```
///
/// Implementing the trait:
///
/// ```rust
/// use tower_resilience_healthcheck::{HealthChecker, HealthStatus};
/// use std::time::Instant;
///
/// struct LatencyChecker {
///     threshold_ms: u64,
/// }
///
/// impl HealthChecker<String> for LatencyChecker {
///     async fn check(&self, resource: &String) -> HealthStatus {
///         let start = Instant::now();
///         // Perform check...
///         let latency = start.elapsed().as_millis() as u64;
///
///         if latency < self.threshold_ms {
///             HealthStatus::Healthy
///         } else {
///             HealthStatus::Degraded
///         }
///     }
/// }
/// ```
pub trait HealthChecker<T>: Send + Sync {
    /// Check the health of the given resource.
    ///
    /// Returns `HealthStatus` indicating the current state.
    fn check(&self, resource: &T) -> impl Future<Output = HealthStatus> + Send;
}

// Blanket implementation for closures - makes it easy to use
impl<T, F, Fut> HealthChecker<T> for F
where
    F: Fn(&T) -> Fut + Send + Sync,
    Fut: Future<Output = HealthStatus> + Send,
{
    fn check(&self, resource: &T) -> impl Future<Output = HealthStatus> + Send {
        self(resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_closure_checker() {
        let checker = |_resource: &String| async { HealthStatus::Healthy };

        let resource = "test".to_string();
        let status = checker.check(&resource).await;
        assert_eq!(status, HealthStatus::Healthy);
    }

    struct AlwaysHealthyChecker;

    impl<T: Sync> HealthChecker<T> for AlwaysHealthyChecker {
        async fn check(&self, _resource: &T) -> HealthStatus {
            HealthStatus::Healthy
        }
    }

    #[tokio::test]
    async fn test_trait_impl_checker() {
        let checker = AlwaysHealthyChecker;
        let resource = "test".to_string();
        let status = checker.check(&resource).await;
        assert_eq!(status, HealthStatus::Healthy);
    }
}
