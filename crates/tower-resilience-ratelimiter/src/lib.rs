//! Advanced rate limiting middleware for Tower services.
//!
//! This crate provides enhanced rate limiting inspired by Resilience4j's RateLimiter,
//! with features beyond Tower's built-in rate limiting.
//!
//! # Features
//!
//! - **Permit-based rate limiting**: Control requests per time period
//! - **Configurable timeout**: Wait up to a specified duration for permits
//! - **Automatic refresh**: Permits automatically refresh after each period
//! - **Event system**: Observability through rate limiter events
//!
//! # Examples
//!
//! ```
//! use tower_resilience_ratelimiter::RateLimiterConfig;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Allow 100 requests per second, wait up to 500ms for a permit
//! let rate_limiter = RateLimiterConfig::builder()
//!     .limit_for_period(100)
//!     .refresh_period(Duration::from_secs(1))
//!     .timeout_duration(Duration::from_millis(500))
//!     .on_permit_acquired(|wait_duration| {
//!         println!("Permit acquired after {:?}", wait_duration);
//!     })
//!     .on_permit_rejected(|timeout| {
//!         println!("Rate limited! Timeout: {:?}", timeout);
//!     })
//!     .build();
//!
//! // Apply to a service
//! let service = ServiceBuilder::new()
//!     .layer(rate_limiter)
//!     .service(tower::service_fn(|req: String| async move {
//!         Ok::<_, std::io::Error>(format!("Response: {}", req))
//!     }));
//! # Ok(())
//! # }
//! ```

mod config;
mod error;
mod events;
mod layer;
mod limiter;

pub use config::{RateLimiterConfig, RateLimiterConfigBuilder};
pub use error::RateLimiterError;
pub use events::RateLimiterEvent;
pub use layer::RateLimiterLayer;

use crate::limiter::SharedRateLimiter;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::Service;

/// A Tower [`Service`] that applies rate limiting.
///
/// This service wraps an inner service and limits the rate at which
/// requests can be processed according to the configured policy.
pub struct RateLimiter<S> {
    inner: S,
    config: Arc<RateLimiterConfig>,
    limiter: SharedRateLimiter,
}

impl<S> RateLimiter<S> {
    /// Creates a new `RateLimiter` wrapping the given service.
    pub fn new(inner: S, config: Arc<RateLimiterConfig>) -> Self {
        let limiter = SharedRateLimiter::new(
            config.limit_for_period,
            config.refresh_period,
            config.timeout_duration,
        );

        Self {
            inner,
            config,
            limiter,
        }
    }
}

impl<S> Clone for RateLimiter<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
            limiter: self.limiter.clone(),
        }
    }
}

impl<S, Req> Service<Req> for RateLimiter<S>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Req: Send + 'static,
{
    type Response = S::Response;
    type Error = RateLimiterError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|_| RateLimiterError::RateLimitExceeded)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let limiter = self.limiter.clone();
        let config = Arc::clone(&self.config);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Try to acquire a permit
            match limiter.acquire().await {
                Ok(wait_duration) => {
                    // Permit acquired
                    let event = RateLimiterEvent::PermitAcquired {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        wait_duration,
                    };
                    config.event_listeners.emit(&event);

                    // Process the request
                    inner
                        .call(req)
                        .await
                        .map_err(|_| RateLimiterError::RateLimitExceeded)
                }
                Err(()) => {
                    // Rate limited
                    let event = RateLimiterEvent::PermitRejected {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        timeout_duration: config.timeout_duration,
                    };
                    config.event_listeners.emit(&event);

                    Err(RateLimiterError::RateLimitExceeded)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::service_fn;
    use tower::{Layer, ServiceExt};

    #[tokio::test]
    async fn test_allows_requests_within_limit() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let layer = RateLimiterConfig::builder()
            .limit_for_period(10)
            .refresh_period(Duration::from_secs(1))
            .timeout_duration(Duration::from_millis(100))
            .build();

        let mut service = layer.layer(service);

        // Should be able to make 10 requests
        for _ in 0..10 {
            let result = service
                .ready()
                .await
                .unwrap()
                .call("test".to_string())
                .await;
            assert!(result.is_ok());
        }

        assert_eq!(call_count.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_rejects_requests_over_limit() {
        let service = service_fn(|req: String| async move {
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        });

        let layer = RateLimiterConfig::builder()
            .limit_for_period(2)
            .refresh_period(Duration::from_secs(10))
            .timeout_duration(Duration::from_millis(10))
            .build();

        let mut service = layer.layer(service);

        // First 2 should succeed
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("1".to_string())
            .await
            .is_ok());
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("2".to_string())
            .await
            .is_ok());

        // Third should be rate limited
        let result = service.ready().await.unwrap().call("3".to_string()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimiterError::RateLimitExceeded
        ));
    }

    #[tokio::test]
    async fn test_permits_refresh_after_period() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("ok".to_string())
            }
        });

        let layer = RateLimiterConfig::builder()
            .limit_for_period(2)
            .refresh_period(Duration::from_millis(100))
            .timeout_duration(Duration::from_millis(200))
            .build();

        let mut service = layer.layer(service);

        // Use up permits
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("1".to_string())
            .await
            .is_ok());
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("2".to_string())
            .await
            .is_ok());

        // Wait for refresh
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be able to make requests again
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("3".to_string())
            .await
            .is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_event_listeners_called() {
        let acquired_count = Arc::new(AtomicUsize::new(0));
        let rejected_count = Arc::new(AtomicUsize::new(0));

        let ac = Arc::clone(&acquired_count);
        let rc = Arc::clone(&rejected_count);

        let service =
            service_fn(|_req: String| async move { Ok::<_, std::io::Error>("ok".to_string()) });

        let layer = RateLimiterConfig::builder()
            .limit_for_period(1)
            .refresh_period(Duration::from_secs(10))
            .timeout_duration(Duration::from_millis(10))
            .on_permit_acquired(move |_| {
                ac.fetch_add(1, Ordering::SeqCst);
            })
            .on_permit_rejected(move |_| {
                rc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        let mut service = layer.layer(service);

        // First request should succeed
        let _ = service.ready().await.unwrap().call("1".to_string()).await;
        assert_eq!(acquired_count.load(Ordering::SeqCst), 1);

        // Second should be rejected
        let _ = service.ready().await.unwrap().call("2".to_string()).await;
        assert_eq!(rejected_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_waits_for_permit_within_timeout() {
        let service =
            service_fn(|_req: String| async move { Ok::<_, std::io::Error>("ok".to_string()) });

        let layer = RateLimiterConfig::builder()
            .limit_for_period(1)
            .refresh_period(Duration::from_millis(50))
            .timeout_duration(Duration::from_millis(100)) // Can wait through one refresh
            .build();

        let mut service = layer.layer(service);

        // First request succeeds
        assert!(service
            .ready()
            .await
            .unwrap()
            .call("1".to_string())
            .await
            .is_ok());

        // Second request should wait for refresh and succeed
        let start = std::time::Instant::now();
        let result = service.ready().await.unwrap().call("2".to_string()).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(elapsed >= Duration::from_millis(45)); // Should have waited
    }
}
