//! Enhanced retry middleware for Tower services.
//!
//! This crate provides advanced retry functionality beyond Tower's built-in retry,
//! with flexible backoff strategies, retry predicates, and comprehensive event system.
//!
//! # Features
//!
//! - **IntervalFunction abstraction**: Pluggable backoff strategies
//!   - Fixed interval
//!   - Exponential backoff with configurable multiplier
//!   - Exponential random backoff with randomization factor
//!   - Custom function-based backoff
//! - **Retry predicates**: Control which errors should be retried
//! - **Event system**: Observability through retry events
//! - **Flexible configuration**: Builder API with sensible defaults
//!
//! # Examples
//!
//! ```
//! use tower_resilience_retry::RetryConfig;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create retry configuration with exponential backoff
//! let retry_config: RetryConfig<MyError> = RetryConfig::builder()
//!     .max_attempts(5)
//!     .exponential_backoff(Duration::from_millis(100))
//!     .on_retry(|attempt, delay| {
//!         println!("Retry attempt {} after {:?}", attempt, delay);
//!     })
//!     .build();
//!
//! // Apply to a service
//! let service = ServiceBuilder::new()
//!     .layer(retry_config.layer())
//!     .service(tower::service_fn(|req: String| async move {
//!         Ok::<_, MyError>(format!("Response: {}", req))
//!     }));
//! # Ok(())
//! # }
//! ```

mod backoff;
mod config;
mod events;
mod layer;
mod policy;

pub use backoff::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, FnInterval, IntervalFunction,
};
pub use config::{RetryConfig, RetryConfigBuilder};
pub use events::RetryEvent;
pub use layer::RetryLayer;
pub use policy::{RetryPolicy, RetryPredicate};

use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::Service;

/// A Tower [`Service`] that retries failed requests.
///
/// This service wraps an inner service and automatically retries requests
/// that fail, according to the configured retry policy and backoff strategy.
pub struct Retry<S, E> {
    inner: S,
    config: Arc<RetryConfig<E>>,
}

impl<S, E> Retry<S, E> {
    /// Creates a new `Retry` service wrapping the given service.
    pub fn new(inner: S, config: Arc<RetryConfig<E>>) -> Self {
        Self { inner, config }
    }
}

impl<S, E> Clone for Retry<S, E>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
        }
    }
}

impl<S, Req, E> Service<Req> for Retry<S, E>
where
    S: Service<Req, Error = E> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Req: Clone + Send + 'static,
    E: Clone + Send + 'static,
    S::Response: Send + 'static,
{
    type Response = S::Response;
    type Error = E;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let mut service = self.inner.clone();
        let config = Arc::clone(&self.config);

        Box::pin(async move {
            let mut attempt = 0;

            loop {
                let result = service.call(req.clone()).await;

                match result {
                    Ok(response) => {
                        // Success
                        let event = RetryEvent::Success {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            attempts: attempt + 1,
                        };
                        config.event_listeners.emit(&event);
                        return Ok(response);
                    }
                    Err(error) => {
                        // Check if we should retry this error
                        if !config.policy.should_retry(&error) {
                            let event = RetryEvent::IgnoredError {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                            };
                            config.event_listeners.emit(&event);
                            return Err(error);
                        }

                        // Check if we've exhausted retries
                        if attempt + 1 >= config.policy.max_attempts {
                            let event = RetryEvent::Error {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                attempts: attempt + 1,
                            };
                            config.event_listeners.emit(&event);
                            return Err(error);
                        }

                        // Calculate backoff and retry
                        let delay = config.policy.next_backoff(attempt);
                        let event = RetryEvent::Retry {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            attempt,
                            delay,
                        };
                        config.event_listeners.emit(&event);

                        tokio::time::sleep(delay).await;
                        attempt += 1;
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tower::service_fn;
    use tower::{Layer, ServiceExt};

    #[derive(Debug, Clone)]
    struct TestError {
        #[allow(dead_code)]
        message: String,
    }

    impl TestError {
        fn new(message: &str) -> Self {
            Self {
                message: message.to_string(),
            }
        }
    }

    #[tokio::test]
    async fn successful_request_no_retry() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(format!("Response: {}", req))
            }
        });

        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

        let layer = config.layer();
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "Response: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_on_failure() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                let count = cc.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err(TestError::new("temporary failure"))
                } else {
                    Ok::<_, TestError>("success".to_string())
                }
            }
        });

        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

        let layer = config.layer();
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "success");
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn exhausts_retries() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(TestError::new("permanent failure"))
            }
        });

        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

        let layer = config.layer();
        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_predicate_filters_errors() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(TestError::new("non-retryable"))
            }
        });

        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .retry_on(|_: &TestError| false) // Never retry
            .build();

        let layer = config.layer();
        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Only called once
    }

    #[tokio::test]
    async fn event_listeners_called() {
        let retry_count = Arc::new(AtomicUsize::new(0));
        let success_count = Arc::new(AtomicUsize::new(0));

        let rc = Arc::clone(&retry_count);
        let sc = Arc::clone(&success_count);

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                let count = cc.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err(TestError::new("temporary"))
                } else {
                    Ok::<_, TestError>("success".to_string())
                }
            }
        });

        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .on_retry(move |_, _| {
                rc.fetch_add(1, Ordering::SeqCst);
            })
            .on_success(move |_| {
                sc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        let layer = config.layer();
        let mut service = layer.layer(service);

        let _ = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert_eq!(retry_count.load(Ordering::SeqCst), 2); // 2 retries
        assert_eq!(success_count.load(Ordering::SeqCst), 1); // 1 success
    }

    #[tokio::test]
    async fn exponential_backoff_increases_delay() {
        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(5)
            .backoff(ExponentialBackoff::new(Duration::from_millis(100)))
            .build();

        assert_eq!(config.policy.next_backoff(0), Duration::from_millis(100));
        assert_eq!(config.policy.next_backoff(1), Duration::from_millis(200));
        assert_eq!(config.policy.next_backoff(2), Duration::from_millis(400));
    }

    #[tokio::test]
    async fn custom_interval_function() {
        let config: RetryConfig<TestError> = RetryConfig::builder()
            .max_attempts(3)
            .backoff(FnInterval::new(|attempt| {
                Duration::from_secs((attempt + 1) as u64)
            }))
            .build();

        assert_eq!(config.policy.next_backoff(0), Duration::from_secs(1));
        assert_eq!(config.policy.next_backoff(1), Duration::from_secs(2));
        assert_eq!(config.policy.next_backoff(2), Duration::from_secs(3));
    }
}
