//! Fallback middleware for Tower services.
//!
//! Provides alternative responses when the inner service fails, enabling
//! graceful degradation and backup service routing.
//!
//! # Overview
//!
//! The fallback pattern allows you to provide alternative responses when your
//! primary service fails. This is useful for:
//!
//! - Returning cached or stale data when the primary source is unavailable
//! - Providing degraded functionality instead of complete failure
//! - Routing to backup services
//! - Transforming errors into user-friendly responses
//!
//! # Fallback Strategies
//!
//! ## Static Value
//!
//! Return a fixed value when the service fails:
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//! use tower::ServiceBuilder;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! let layer = FallbackLayer::<String, String, MyError>::value("default response".to_string());
//! ```
//!
//! ## Fallback Function
//!
//! Compute a response based on the error:
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError { message: String }
//! let layer = FallbackLayer::<String, String, MyError>::from_error(|error: &MyError| {
//!     format!("Service unavailable: {}", error.message)
//! });
//! ```
//!
//! ## Fallback with Request Context
//!
//! Access both the original request and error:
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//! use std::sync::Arc;
//! use std::collections::HashMap;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! let cache: Arc<HashMap<String, String>> = Arc::new(HashMap::new());
//! let cache_clone = Arc::clone(&cache);
//!
//! let layer = FallbackLayer::<String, String, MyError>::from_request_error(move |req: &String, _err: &MyError| {
//!     cache_clone.get(req).cloned().unwrap_or_else(|| "not found".to_string())
//! });
//! ```
//!
//! ## Backup Service
//!
//! Route to an alternative service (async):
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! let layer = FallbackLayer::<String, String, MyError>::service(|req: String| async move {
//!     Ok::<_, MyError>(format!("backup: {}", req))
//! });
//! ```
//!
//! ## Error Transformation
//!
//! Convert errors to a different type (still returns error, not success):
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//!
//! # #[derive(Debug, Clone)]
//! # struct InternalError { code: u32 }
//! // Note: This changes the error type, so layer types must align
//! let layer = FallbackLayer::<String, String, InternalError>::exception(|err: InternalError| {
//!     InternalError { code: 500 } // Transform but still error
//! });
//! ```
//!
//! # Selective Error Handling
//!
//! Only trigger fallback for specific errors:
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError { retryable: bool }
//! let layer: FallbackLayer<String, String, MyError> = FallbackLayer::builder()
//!     .value("fallback".to_string())
//!     .handle(|e: &MyError| e.retryable) // Only fallback on retryable errors
//!     .build();
//! ```
//!
//! # Composition with Other Layers
//!
//! Fallback works well with other resilience patterns:
//!
//! ```rust
//! use tower_resilience_fallback::FallbackLayer;
//! use tower::ServiceBuilder;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # async fn example() {
//! // Fallback catches anything that escapes retry + circuit breaker
//! let service = ServiceBuilder::new()
//!     .layer(FallbackLayer::<String, String, MyError>::value("unavailable".to_string()))
//!     // .layer(CircuitBreakerLayer::new(cb_config))
//!     // .layer(RetryLayer::new(retry_config))
//!     .service(tower::service_fn(|req: String| async move {
//!         Ok::<_, MyError>(req)
//!     }));
//! # }
//! ```
//!
//! # Events
//!
//! The fallback service emits events for observability:
//!
//! - `Success`: Inner service succeeded, no fallback needed
//! - `FailedAttempt`: Inner service failed, fallback will be attempted
//! - `Applied`: Fallback was successfully applied
//! - `Failed`: Fallback itself failed (service fallback only)
//! - `Skipped`: Error didn't match predicate, propagated as-is

mod config;
mod error;
mod events;
mod layer;

pub use config::{FallbackConfig, FallbackConfigBuilder};
pub use error::FallbackError;
pub use events::FallbackEvent;
pub use layer::FallbackLayer;

use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter};

#[cfg(feature = "metrics")]
use std::sync::Once;

#[cfg(feature = "metrics")]
static METRICS_INIT: Once = Once::new();

/// Function that computes a fallback response from an error.
pub type FromErrorFn<Res, E> = Arc<dyn Fn(&E) -> Res + Send + Sync>;

/// Function that computes a fallback response from request and error.
pub type FromRequestErrorFn<Req, Res, E> = Arc<dyn Fn(&Req, &E) -> Res + Send + Sync>;

/// Function that calls a backup service asynchronously.
pub type ServiceFn<Req, Res, E> =
    Arc<dyn Fn(Req) -> BoxFuture<'static, Result<Res, E>> + Send + Sync>;

/// Function that transforms an error into a different error.
pub type ExceptionFn<E> = Arc<dyn Fn(E) -> E + Send + Sync>;

/// The strategy used to produce a fallback response.
pub enum FallbackStrategy<Req, Res, E> {
    /// Return a static value (cloned for each fallback).
    Value(Res),

    /// Compute a response from the error.
    FromError(FromErrorFn<Res, E>),

    /// Compute a response from both the request and error.
    FromRequestError(FromRequestErrorFn<Req, Res, E>),

    /// Call a backup service asynchronously.
    /// The function takes the request and returns a future.
    Service(ServiceFn<Req, Res, E>),

    /// Transform the error into a different error (still fails, but with transformed error).
    Exception(ExceptionFn<E>),
}

impl<Req, Res, E> Clone for FallbackStrategy<Req, Res, E>
where
    Res: Clone,
{
    fn clone(&self) -> Self {
        match self {
            Self::Value(v) => Self::Value(v.clone()),
            Self::FromError(f) => Self::FromError(Arc::clone(f)),
            Self::FromRequestError(f) => Self::FromRequestError(Arc::clone(f)),
            Self::Service(s) => Self::Service(Arc::clone(s)),
            Self::Exception(f) => Self::Exception(Arc::clone(f)),
        }
    }
}

/// Predicate to determine if an error should trigger the fallback.
pub type HandlePredicate<E> = Arc<dyn Fn(&E) -> bool + Send + Sync>;

/// A Tower service that provides fallback responses when the inner service fails.
///
/// See the [module-level documentation](crate) for usage examples.
pub struct Fallback<S, Req, Res, E> {
    inner: S,
    config: Arc<FallbackConfig<Req, Res, E>>,
}

impl<S, Req, Res, E> Fallback<S, Req, Res, E> {
    /// Creates a new `Fallback` service wrapping the given service.
    pub fn new(inner: S, config: Arc<FallbackConfig<Req, Res, E>>) -> Self {
        #[cfg(feature = "metrics")]
        METRICS_INIT.call_once(|| {
            describe_counter!(
                "fallback_calls_total",
                "Total number of fallback operations"
            );
        });

        Self { inner, config }
    }
}

impl<S, Req, Res, E> Clone for Fallback<S, Req, Res, E>
where
    S: Clone,
    Res: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
        }
    }
}

impl<S, Req, Res, E> Service<Req> for Fallback<S, Req, Res, E>
where
    S: Service<Req, Response = Res, Error = E> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Req: Clone + Send + Sync + 'static,
    Res: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    type Response = Res;
    type Error = FallbackError<E>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(FallbackError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let mut service = self.inner.clone();
        let config = Arc::clone(&self.config);
        let req_clone = req.clone();

        Box::pin(async move {
            #[cfg(feature = "tracing")]
            tracing::debug!(fallback = %config.name, "Calling inner service");

            let result = service.call(req).await;

            match result {
                Ok(response) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(fallback = %config.name, "Inner service succeeded");

                    #[cfg(feature = "metrics")]
                    counter!(
                        "fallback_calls_total",
                        "fallback" => config.name.clone(),
                        "result" => "success"
                    )
                    .increment(1);

                    let event = FallbackEvent::Success {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                    };
                    config.event_listeners.emit(&event);

                    Ok(response)
                }
                Err(error) => {
                    // Check if we should handle this error
                    let should_handle = config
                        .handle_predicate
                        .as_ref()
                        .map(|p| p(&error))
                        .unwrap_or(true);

                    if !should_handle {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(
                            fallback = %config.name,
                            "Error does not match predicate, skipping fallback"
                        );

                        #[cfg(feature = "metrics")]
                        counter!(
                            "fallback_calls_total",
                            "fallback" => config.name.clone(),
                            "result" => "skipped"
                        )
                        .increment(1);

                        let event = FallbackEvent::Skipped {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                        };
                        config.event_listeners.emit(&event);

                        return Err(FallbackError::Inner(error));
                    }

                    #[cfg(feature = "tracing")]
                    tracing::debug!(fallback = %config.name, "Inner service failed, applying fallback");

                    // Emit failed attempt event
                    let event = FallbackEvent::FailedAttempt {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                    };
                    config.event_listeners.emit(&event);

                    // Apply fallback strategy
                    match &config.strategy {
                        FallbackStrategy::Value(v) => {
                            #[cfg(feature = "metrics")]
                            counter!(
                                "fallback_calls_total",
                                "fallback" => config.name.clone(),
                                "result" => "applied",
                                "strategy" => "value"
                            )
                            .increment(1);

                            let event = FallbackEvent::Applied {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                strategy: "value",
                            };
                            config.event_listeners.emit(&event);

                            Ok(v.clone())
                        }

                        FallbackStrategy::FromError(f) => {
                            let response = f(&error);

                            #[cfg(feature = "metrics")]
                            counter!(
                                "fallback_calls_total",
                                "fallback" => config.name.clone(),
                                "result" => "applied",
                                "strategy" => "from_error"
                            )
                            .increment(1);

                            let event = FallbackEvent::Applied {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                strategy: "from_error",
                            };
                            config.event_listeners.emit(&event);

                            Ok(response)
                        }

                        FallbackStrategy::FromRequestError(f) => {
                            let response = f(&req_clone, &error);

                            #[cfg(feature = "metrics")]
                            counter!(
                                "fallback_calls_total",
                                "fallback" => config.name.clone(),
                                "result" => "applied",
                                "strategy" => "from_request_error"
                            )
                            .increment(1);

                            let event = FallbackEvent::Applied {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                strategy: "from_request_error",
                            };
                            config.event_listeners.emit(&event);

                            Ok(response)
                        }

                        FallbackStrategy::Service(backup) => {
                            #[cfg(feature = "tracing")]
                            tracing::debug!(fallback = %config.name, "Calling backup service");

                            match backup(req_clone).await {
                                Ok(response) => {
                                    #[cfg(feature = "metrics")]
                                    counter!(
                                        "fallback_calls_total",
                                        "fallback" => config.name.clone(),
                                        "result" => "applied",
                                        "strategy" => "service"
                                    )
                                    .increment(1);

                                    let event = FallbackEvent::Applied {
                                        pattern_name: config.name.clone(),
                                        timestamp: Instant::now(),
                                        strategy: "service",
                                    };
                                    config.event_listeners.emit(&event);

                                    Ok(response)
                                }
                                Err(backup_error) => {
                                    #[cfg(feature = "tracing")]
                                    tracing::warn!(
                                        fallback = %config.name,
                                        "Backup service also failed"
                                    );

                                    #[cfg(feature = "metrics")]
                                    counter!(
                                        "fallback_calls_total",
                                        "fallback" => config.name.clone(),
                                        "result" => "failed",
                                        "strategy" => "service"
                                    )
                                    .increment(1);

                                    let event = FallbackEvent::Failed {
                                        pattern_name: config.name.clone(),
                                        timestamp: Instant::now(),
                                    };
                                    config.event_listeners.emit(&event);

                                    Err(FallbackError::FallbackFailed(backup_error))
                                }
                            }
                        }

                        FallbackStrategy::Exception(transform) => {
                            let transformed = transform(error);

                            #[cfg(feature = "metrics")]
                            counter!(
                                "fallback_calls_total",
                                "fallback" => config.name.clone(),
                                "result" => "transformed",
                                "strategy" => "exception"
                            )
                            .increment(1);

                            let event = FallbackEvent::Applied {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                strategy: "exception",
                            };
                            config.event_listeners.emit(&event);

                            Err(FallbackError::Inner(transformed))
                        }
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
    use tower::{service_fn, Layer, ServiceExt};
    use tower_resilience_core::ResilienceEvent;

    #[derive(Debug, Clone)]
    struct TestError {
        message: String,
        retryable: bool,
    }

    impl TestError {
        fn new(message: &str) -> Self {
            Self {
                message: message.to_string(),
                retryable: true,
            }
        }

        fn non_retryable(message: &str) -> Self {
            Self {
                message: message.to_string(),
                retryable: false,
            }
        }
    }

    #[tokio::test]
    async fn success_no_fallback() {
        let service =
            service_fn(
                |req: String| async move { Ok::<_, TestError>(format!("response: {}", req)) },
            );

        let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "response: test");
    }

    #[tokio::test]
    async fn failure_triggers_value_fallback() {
        let service =
            service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

        let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "fallback");
    }

    #[tokio::test]
    async fn failure_triggers_from_error_fallback() {
        let service = service_fn(|_req: String| async move {
            Err::<String, _>(TestError::new("something went wrong"))
        });

        let layer = FallbackLayer::<String, String, TestError>::from_error(|e: &TestError| {
            format!("Error: {}", e.message)
        });
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "Error: something went wrong");
    }

    #[tokio::test]
    async fn failure_triggers_from_request_error_fallback() {
        let service =
            service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

        let layer = FallbackLayer::<String, String, TestError>::from_request_error(
            |req: &String, _e: &TestError| format!("fallback for: {}", req),
        );
        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("my-request".to_string())
            .await
            .unwrap();

        assert_eq!(response, "fallback for: my-request");
    }

    #[tokio::test]
    async fn predicate_skips_non_matching_errors() {
        let service = service_fn(|_req: String| async move {
            Err::<String, _>(TestError::non_retryable("permanent failure"))
        });

        let layer = FallbackLayer::builder()
            .value("fallback".to_string())
            .handle(|e: &TestError| e.retryable) // Only handle retryable errors
            .build();
        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        // Should propagate the original error, not apply fallback
        assert!(matches!(result, Err(FallbackError::Inner(_))));
    }

    #[tokio::test]
    async fn backup_service_fallback() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let primary = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(TestError::new("primary failed"))
            }
        });

        let backup_calls = Arc::new(AtomicUsize::new(0));
        let bc = Arc::clone(&backup_calls);

        let layer = FallbackLayer::<String, String, TestError>::service(move |req: String| {
            let bc = Arc::clone(&bc);
            async move {
                bc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(format!("backup: {}", req))
            }
        });
        let mut service = layer.layer(primary);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "backup: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        assert_eq!(backup_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn backup_service_also_fails() {
        let primary =
            service_fn(
                |_req: String| async move { Err::<String, _>(TestError::new("primary failed")) },
            );

        let layer =
            FallbackLayer::<String, String, TestError>::service(|_req: String| async move {
                Err::<String, _>(TestError::new("backup also failed"))
            });
        let mut service = layer.layer(primary);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert!(matches!(result, Err(FallbackError::FallbackFailed(_))));
    }

    #[tokio::test]
    async fn exception_transforms_error() {
        let service =
            service_fn(
                |_req: String| async move { Err::<String, _>(TestError::new("original error")) },
            );

        let layer = FallbackLayer::<String, String, TestError>::exception(|_e: TestError| {
            TestError::new("transformed error")
        });
        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        match result {
            Err(FallbackError::Inner(e)) => {
                assert_eq!(e.message, "transformed error");
            }
            _ => panic!("expected transformed error"),
        }
    }

    #[tokio::test]
    async fn event_listeners_called() {
        use std::sync::Mutex;

        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);

        let service =
            service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

        let layer = FallbackLayer::builder()
            .name("test-fallback")
            .value("fallback".to_string())
            .on_event(move |event: &FallbackEvent| {
                events_clone
                    .lock()
                    .unwrap()
                    .push(event.event_type().to_string());
            })
            .build();
        let mut service = layer.layer(service);

        let _ = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        let recorded = events.lock().unwrap();
        assert!(recorded.contains(&"failed_attempt".to_string()));
        assert!(recorded.contains(&"applied".to_string()));
    }
}
