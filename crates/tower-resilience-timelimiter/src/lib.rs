//! Advanced timeout handling for Tower services.
//!
//! Provides timeout functionality with:
//! - Configurable timeout duration
//! - Optional future cancellation on timeout
//! - Event system for observability (onSuccess, onError, onTimeout)
//! - Metrics integration
//!
//! ## Basic Example
//!
//! ```rust
//! use tower_resilience_timelimiter::TimeLimiterLayer;
//! use tower::{Layer, service_fn};
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = TimeLimiterLayer::builder()
//!     .timeout_duration(Duration::from_secs(5))
//!     .cancel_running_future(true)
//!     .on_timeout(|| {
//!         eprintln!("Request timed out!");
//!     })
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     Ok::<String, ()>(req)
//! });
//!
//! let mut service = layer.layer(svc);
//! # }
//! ```
//!
//! ## Event Listeners
//!
//! ```rust
//! use tower_resilience_timelimiter::TimeLimiterLayer;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = TimeLimiterLayer::builder()
//!     .timeout_duration(Duration::from_secs(5))
//!     .on_success(|duration| {
//!         println!("Call succeeded in {:?}", duration);
//!     })
//!     .on_error(|duration| {
//!         println!("Call failed after {:?}", duration);
//!     })
//!     .on_timeout(|| {
//!         println!("Call timed out");
//!     })
//!     .build();
//! # }
//! ```
//!
//! ## Fallback on Timeout
//!
//! Handle timeout errors with fallback strategies:
//!
//! ### Return Partial Results
//!
//! ```rust
//! use tower_resilience_timelimiter::{TimeLimiterLayer, TimeLimiterError};
//! use tower::{Layer, Service, ServiceExt, service_fn};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let layer = TimeLimiterLayer::builder()
//!     .timeout_duration(Duration::from_millis(100))
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     tokio::time::sleep(Duration::from_secs(1)).await;
//!     Ok::<String, std::io::Error>(format!("Full result: {}", req))
//! });
//!
//! let mut service = layer.layer(svc);
//!
//! let result = service.ready().await?.call("data".to_string()).await
//!     .unwrap_or_else(|_| "Partial result: using cached data".to_string());
//! # Ok(())
//! # }
//! ```
//!
//! ### Return Cached Data
//!
//! ```rust
//! use tower_resilience_timelimiter::{TimeLimiterLayer, TimeLimiterError};
//! use tower::{Layer, Service, ServiceExt, service_fn};
//! use std::time::Duration;
//! use std::sync::Arc;
//! use std::collections::HashMap;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let cache = Arc::new(std::sync::RwLock::new(HashMap::new()));
//! cache.write().unwrap().insert("key", "cached value");
//!
//! let layer = TimeLimiterLayer::builder()
//!     .timeout_duration(Duration::from_millis(100))
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     tokio::time::sleep(Duration::from_secs(1)).await;
//!     Ok::<String, std::io::Error>(req)
//! });
//!
//! let mut service = layer.layer(svc);
//! let cache_clone = Arc::clone(&cache);
//!
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
//!
//! ### Informative Timeout Message
//!
//! ```rust
//! use tower_resilience_timelimiter::{TimeLimiterLayer, TimeLimiterError};
//! use tower::{Layer, Service, ServiceExt, service_fn};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let layer = TimeLimiterLayer::builder()
//!     .timeout_duration(Duration::from_millis(100))
//!     .on_timeout(|| {
//!         eprintln!("Operation timed out - this may indicate service degradation");
//!     })
//!     .build();
//!
//! let svc = service_fn(|req: String| async move {
//!     tokio::time::sleep(Duration::from_secs(1)).await;
//!     Ok::<String, std::io::Error>(req)
//! });
//!
//! let mut service = layer.layer(svc);
//!
//! match service.ready().await?.call("request".to_string()).await {
//!     Ok(response) => println!("Success: {}", response),
//!     Err(_) => println!("Request timed out - please try again or contact support"),
//! }
//! # Ok(())
//! # }
//! ```

use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::time::timeout;
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter, describe_histogram, histogram};

#[cfg(feature = "tracing")]
use tracing::{debug, warn};

pub use config::{TimeLimiterConfig, TimeLimiterConfigBuilder};
pub use error::TimeLimiterError;
pub use events::TimeLimiterEvent;
pub use layer::TimeLimiterLayer;

mod config;
mod error;
mod events;
mod layer;

/// A Tower service that applies timeout limiting to an inner service.
#[derive(Clone)]
pub struct TimeLimiter<S> {
    inner: S,
    config: Arc<TimeLimiterConfig>,
}

impl<S> TimeLimiter<S> {
    /// Creates a new time limiter wrapping the given service.
    pub(crate) fn new(inner: S, config: Arc<TimeLimiterConfig>) -> Self {
        #[cfg(feature = "metrics")]
        {
            describe_counter!(
                "timelimiter_calls_total",
                "Total number of time limiter calls (success, error, or timeout)"
            );
            describe_histogram!(
                "timelimiter_call_duration_seconds",
                "Duration of calls (successful or failed)"
            );
        }

        Self { inner, config }
    }
}

impl<S, Request> Service<Request> for TimeLimiter<S>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = TimeLimiterError<S::Error>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(TimeLimiterError::Inner)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let config = Arc::clone(&self.config);
        let timeout_duration = config.timeout_duration;

        Box::pin(async move {
            let start = Instant::now();

            match timeout(timeout_duration, inner.call(req)).await {
                Ok(Ok(response)) => {
                    let duration = start.elapsed();
                    config.event_listeners.emit(&TimeLimiterEvent::Success {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        duration,
                    });

                    #[cfg(feature = "metrics")]
                    {
                        counter!("timelimiter_calls_total", "timelimiter" => config.name.clone(), "result" => "success").increment(1);
                        histogram!("timelimiter_call_duration_seconds", "timelimiter" => config.name.clone())
                            .record(duration.as_secs_f64());
                    }

                    #[cfg(feature = "tracing")]
                    debug!(
                        timelimiter = %config.name,
                        duration_ms = duration.as_millis(),
                        "Call succeeded within timeout"
                    );

                    Ok(response)
                }
                Ok(Err(err)) => {
                    let duration = start.elapsed();
                    config.event_listeners.emit(&TimeLimiterEvent::Error {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        duration,
                    });

                    #[cfg(feature = "metrics")]
                    {
                        counter!("timelimiter_calls_total", "timelimiter" => config.name.clone(), "result" => "error").increment(1);
                        histogram!("timelimiter_call_duration_seconds", "timelimiter" => config.name.clone())
                            .record(duration.as_secs_f64());
                    }

                    #[cfg(feature = "tracing")]
                    debug!(
                        timelimiter = %config.name,
                        duration_ms = duration.as_millis(),
                        "Call failed within timeout"
                    );

                    Err(TimeLimiterError::Inner(err))
                }
                Err(_elapsed) => {
                    config.event_listeners.emit(&TimeLimiterEvent::Timeout {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        timeout_duration,
                    });

                    #[cfg(feature = "metrics")]
                    {
                        counter!("timelimiter_calls_total", "timelimiter" => config.name.clone(), "result" => "timeout").increment(1);
                    }

                    #[cfg(feature = "tracing")]
                    warn!(
                        timelimiter = %config.name,
                        timeout_ms = timeout_duration.as_millis(),
                        "Call timed out"
                    );

                    Err(TimeLimiterError::Timeout)
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
    use tokio::time::sleep;
    use tower::{service_fn, Layer, ServiceExt};

    #[tokio::test]
    async fn test_success_within_timeout() {
        let layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(100))
            .build();

        let svc = service_fn(|_req: ()| async {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, ()>("success")
        });

        let mut service = layer.layer(svc);
        let result = service.ready().await.unwrap().call(()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_timeout_occurs() {
        let layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(10))
            .build();

        let svc = service_fn(|_req: ()| async {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, ()>("success")
        });

        let mut service = layer.layer(svc);
        let result = service.ready().await.unwrap().call(()).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_timeout());
    }

    #[tokio::test]
    async fn test_inner_error_propagates() {
        let layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(100))
            .build();

        let svc = service_fn(|_req: ()| async { Err::<(), _>("inner error") });

        let mut service = layer.layer(svc);
        let result = service.ready().await.unwrap().call(()).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(!err.is_timeout());
        assert_eq!(err.into_inner(), Some("inner error"));
    }

    #[tokio::test]
    async fn test_event_listeners() {
        let success_count = Arc::new(AtomicUsize::new(0));
        let timeout_count = Arc::new(AtomicUsize::new(0));

        let sc = Arc::clone(&success_count);
        let tc = Arc::clone(&timeout_count);

        let layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(50))
            .on_success(move |_| {
                sc.fetch_add(1, Ordering::SeqCst);
            })
            .on_timeout(move || {
                tc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        // Test success
        let svc = service_fn(|_req: ()| async {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, ()>("ok")
        });
        let mut service = layer.layer(svc);
        let _ = service.ready().await.unwrap().call(()).await;
        assert_eq!(success_count.load(Ordering::SeqCst), 1);

        // Test timeout
        let svc = service_fn(|_req: ()| async {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, ()>("ok")
        });
        let mut service = layer.layer(svc);
        let _ = service.ready().await.unwrap().call(()).await;
        assert_eq!(timeout_count.load(Ordering::SeqCst), 1);
    }

    // Note: The cancel_running_future flag is tested in tests/timelimiter/cancellation.rs
}
