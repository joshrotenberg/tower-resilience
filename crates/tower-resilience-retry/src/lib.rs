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
//! - **Per-request configuration**: Extract max attempts from the request
//! - **Retry predicates**: Control which errors should be retried
//! - **Event system**: Observability through retry events
//! - **Flexible configuration**: Builder API with sensible defaults
//!
//! # Examples
//!
//! ## Basic Retry with Exponential Backoff
//!
//! ```
//! use tower_resilience_retry::RetryLayer;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create retry layer with exponential backoff
//! let retry_layer = RetryLayer::<String, MyError>::builder()
//!     .max_attempts(5)
//!     .exponential_backoff(Duration::from_millis(100))
//!     .on_retry(|attempt, delay| {
//!         println!("Retry attempt {} after {:?}", attempt, delay);
//!     })
//!     .build();
//!
//! // Apply to a service
//! let service = ServiceBuilder::new()
//!     .layer(retry_layer)
//!     .service(tower::service_fn(|req: String| async move {
//!         Ok::<_, MyError>(format!("Response: {}", req))
//!     }));
//! # Ok(())
//! # }
//! ```
//!
//! ## Per-Request Max Attempts
//!
//! Extract retry configuration from the request itself:
//!
//! ```
//! use tower_resilience_retry::RetryLayer;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! #[derive(Clone)]
//! struct MyRequest {
//!     is_idempotent: bool,
//!     data: String,
//! }
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # async fn example() {
//! // Idempotent requests can retry more aggressively
//! let retry_layer = RetryLayer::<MyRequest, MyError>::builder()
//!     .max_attempts_fn(|req: &MyRequest| {
//!         if req.is_idempotent { 5 } else { 1 }
//!     })
//!     .exponential_backoff(Duration::from_millis(100))
//!     .build();
//! # }
//! ```
//!
//! ## Fallback After Retry Exhaustion
//!
//! When retries are exhausted, you can provide a fallback response using standard error handling:
//!
//! ```
//! use tower_resilience_retry::RetryLayer;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use std::time::Duration;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # impl std::fmt::Display for MyError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//! #         write!(f, "MyError")
//! #     }
//! # }
//! # impl std::error::Error for MyError {}
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let retry_layer = RetryLayer::<String, MyError>::builder()
//!     .max_attempts(3)
//!     .exponential_backoff(Duration::from_millis(100))
//!     .build();
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(retry_layer)
//!     .service(tower::service_fn(|req: String| async move {
//!         Err::<String, MyError>(MyError) // Always fails
//!     }));
//!
//! // Handle retry exhaustion with fallback
//! let result = service.ready().await?.call("request".to_string()).await
//!     .unwrap_or_else(|_| "Fallback: Service unavailable".to_string());
//! # Ok(())
//! # }
//! ```
//!
//! ### Fallback with Cached Data
//!
//! ```
//! use tower_resilience_retry::RetryLayer;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use std::time::Duration;
//! use std::sync::Arc;
//! use std::collections::HashMap;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # impl std::fmt::Display for MyError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//! #         write!(f, "MyError")
//! #     }
//! # }
//! # impl std::error::Error for MyError {}
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let cache = Arc::new(std::sync::RwLock::new(HashMap::new()));
//! cache.write().unwrap().insert("key", "cached value");
//!
//! let retry_layer = RetryLayer::<String, MyError>::builder()
//!     .max_attempts(3)
//!     .exponential_backoff(Duration::from_millis(50))
//!     .build();
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(retry_layer)
//!     .service(tower::service_fn(|req: String| async move {
//!         Err::<String, MyError>(MyError)
//!     }));
//!
//! let cache_clone = Arc::clone(&cache);
//! let result = service.ready().await?.call("key".to_string()).await
//!     .unwrap_or_else(|_| {
//!         cache_clone.read().unwrap()
//!             .get("key")
//!             .map(|s| s.to_string())
//!             .unwrap_or_else(|| "Default value".to_string())
//!     });
//! # Ok(())
//! # }
//! ```

mod backoff;
mod budget;
mod config;
mod events;
mod layer;
mod policy;

pub use backoff::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, FnInterval, IntervalFunction,
};
pub use budget::{AimdBudget, RetryBudget, RetryBudgetBuilder, TokenBucketBudget};
pub use config::{MaxAttemptsSource, RetryConfig, RetryConfigBuilder};
pub use events::RetryEvent;
pub use layer::RetryLayer;
pub use policy::{RetryPolicy, RetryPredicate};

use futures::future::BoxFuture;
use std::marker::PhantomData;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter, describe_histogram, histogram};

#[cfg(feature = "tracing")]
use tracing::{debug, info, warn};

/// A Tower [`Service`] that retries failed requests.
///
/// This service wraps an inner service and automatically retries requests
/// that fail, according to the configured retry policy and backoff strategy.
pub struct Retry<S, Req, E> {
    inner: S,
    config: Arc<RetryConfig<Req, E>>,
    _phantom: PhantomData<Req>,
}

impl<S, Req, E> Retry<S, Req, E> {
    /// Creates a new `Retry` service wrapping the given service.
    pub fn new(inner: S, config: Arc<RetryConfig<Req, E>>, _phantom: PhantomData<Req>) -> Self {
        #[cfg(feature = "metrics")]
        {
            describe_counter!(
                "retry_calls_total",
                "Total number of retry operations (success or exhausted)"
            );
            describe_counter!(
                "retry_attempts_total",
                "Total number of retry attempts across all calls"
            );
            describe_histogram!("retry_attempts", "Number of attempts per successful call");
        }

        Self {
            inner,
            config,
            _phantom,
        }
    }
}

impl<S, Req, E> Clone for Retry<S, Req, E>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
            _phantom: PhantomData,
        }
    }
}

impl<S, Req, E> Service<Req> for Retry<S, Req, E>
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

        // Extract max_attempts from request before moving it
        let max_attempts = config.max_attempts_source.get_max_attempts(&req);

        Box::pin(async move {
            let mut attempt = 0;

            loop {
                let result = service.call(req.clone()).await;

                match result {
                    Ok(response) => {
                        // Success - deposit to budget if configured
                        if let Some(ref budget) = config.budget {
                            budget.deposit();
                        }

                        #[cfg(feature = "metrics")]
                        {
                            counter!("retry_calls_total", "retry" => config.name.clone(), "result" => "success").increment(1);
                            histogram!("retry_attempts", "retry" => config.name.clone())
                                .record((attempt + 1) as f64);
                        }

                        #[cfg(feature = "tracing")]
                        {
                            if attempt > 0 {
                                info!(retry = %config.name, attempts = attempt + 1, "Request succeeded after retries");
                            } else {
                                debug!(retry = %config.name, "Request succeeded on first attempt");
                            }
                        }

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
                            #[cfg(feature = "tracing")]
                            debug!(retry = %config.name, "Error not retryable, failing immediately");

                            let event = RetryEvent::IgnoredError {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                            };
                            config.event_listeners.emit(&event);
                            return Err(error);
                        }

                        // Check if we've exhausted retries (use per-request max_attempts)
                        if attempt + 1 >= max_attempts {
                            #[cfg(feature = "metrics")]
                            {
                                counter!("retry_calls_total", "retry" => config.name.clone(), "result" => "exhausted").increment(1);
                            }

                            #[cfg(feature = "tracing")]
                            warn!(retry = %config.name, attempts = attempt + 1, max_attempts = max_attempts, "Retry attempts exhausted");

                            let event = RetryEvent::Error {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                attempts: attempt + 1,
                            };
                            config.event_listeners.emit(&event);
                            return Err(error);
                        }

                        // Check retry budget if configured
                        if let Some(ref budget) = config.budget {
                            if !budget.try_withdraw() {
                                #[cfg(feature = "metrics")]
                                {
                                    counter!("retry_calls_total", "retry" => config.name.clone(), "result" => "budget_exhausted").increment(1);
                                }

                                #[cfg(feature = "tracing")]
                                warn!(retry = %config.name, attempt = attempt + 1, "Retry budget exhausted, failing immediately");

                                let event = RetryEvent::BudgetExhausted {
                                    pattern_name: config.name.clone(),
                                    timestamp: Instant::now(),
                                    attempt: attempt + 1,
                                };
                                config.event_listeners.emit(&event);
                                return Err(error);
                            }
                        }

                        // Calculate backoff and retry
                        let delay = config.policy.next_backoff(attempt);

                        #[cfg(feature = "metrics")]
                        {
                            counter!("retry_attempts_total", "retry" => config.name.clone())
                                .increment(1);
                        }

                        #[cfg(feature = "tracing")]
                        debug!(retry = %config.name, attempt = attempt + 1, delay_ms = delay.as_millis(), "Retrying after delay");

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

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

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

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

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

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .build();

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

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .retry_on(|_: &TestError| false) // Never retry
            .build();

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

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(3)
            .fixed_backoff(Duration::from_millis(10))
            .on_retry(move |_, _| {
                rc.fetch_add(1, Ordering::SeqCst);
            })
            .on_success(move |_| {
                sc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

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
    async fn budget_limits_retries() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let budget_exhausted_count = Arc::new(AtomicUsize::new(0));

        let cc = Arc::clone(&call_count);
        let bec = Arc::clone(&budget_exhausted_count);

        // Create a budget with only 1 token
        let budget = RetryBudgetBuilder::new()
            .token_bucket()
            .max_tokens(1)
            .initial_tokens(1)
            .build();

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(TestError::new("always fails"))
            }
        });

        let layer = RetryLayer::<String, TestError>::builder()
            .max_attempts(5)
            .fixed_backoff(Duration::from_millis(1))
            .budget(budget)
            .on_budget_exhausted(move |_| {
                bec.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert!(result.is_err());
        // Should have called twice: 1 initial + 1 retry (budget allows 1 retry)
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        // Budget exhausted should be called once (when 2nd retry was blocked)
        assert_eq!(budget_exhausted_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn budget_replenishes_on_success() {
        let budget = RetryBudgetBuilder::new()
            .token_bucket()
            .max_tokens(10)
            .initial_tokens(0) // Start empty
            .build();

        // Budget starts empty
        assert_eq!(budget.balance(), 0);
        assert!(!budget.try_withdraw());

        // Deposit (simulating successful request)
        budget.deposit();
        assert_eq!(budget.balance(), 1);

        // Now withdrawal should work
        assert!(budget.try_withdraw());
        assert_eq!(budget.balance(), 0);
    }

    #[tokio::test]
    async fn per_request_max_attempts() {
        #[derive(Clone)]
        struct Request {
            is_idempotent: bool,
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: Request| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(TestError::new("always fails"))
            }
        });

        let layer = RetryLayer::<Request, TestError>::builder()
            .max_attempts_fn(|req: &Request| if req.is_idempotent { 5 } else { 1 })
            .fixed_backoff(Duration::from_millis(1))
            .build();

        let mut service = layer.layer(service);

        // Non-idempotent request - should only try once
        call_count.store(0, Ordering::SeqCst);
        let _ = service
            .ready()
            .await
            .unwrap()
            .call(Request {
                is_idempotent: false,
            })
            .await;
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Idempotent request - should try 5 times
        call_count.store(0, Ordering::SeqCst);
        let _ = service
            .ready()
            .await
            .unwrap()
            .call(Request {
                is_idempotent: true,
            })
            .await;
        assert_eq!(call_count.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn per_request_max_attempts_with_success() {
        #[derive(Clone)]
        struct Request {
            max_retries: usize,
            succeed_on_attempt: usize,
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: Request| {
            let cc = Arc::clone(&cc);
            async move {
                let attempt = cc.fetch_add(1, Ordering::SeqCst);
                if attempt >= req.succeed_on_attempt {
                    Ok::<_, TestError>("success".to_string())
                } else {
                    Err(TestError::new("not yet"))
                }
            }
        });

        let layer = RetryLayer::<Request, TestError>::builder()
            .max_attempts_fn(|req: &Request| req.max_retries)
            .fixed_backoff(Duration::from_millis(1))
            .build();

        let mut service = layer.layer(service);

        // Request that succeeds on 3rd attempt with 5 max retries
        call_count.store(0, Ordering::SeqCst);
        let result = service
            .ready()
            .await
            .unwrap()
            .call(Request {
                max_retries: 5,
                succeed_on_attempt: 2,
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 3);

        // Request that would need 3 attempts but only has 2 max
        call_count.store(0, Ordering::SeqCst);
        let result = service
            .ready()
            .await
            .unwrap()
            .call(Request {
                max_retries: 2,
                succeed_on_attempt: 2,
            })
            .await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    // Note: Backoff behavior is tested in tests/retry/retry_backoff.rs
}
