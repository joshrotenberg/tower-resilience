//! Tower layer for time limiter.

use crate::config::{FixedTimeout, TimeLimiterConfig};
use crate::TimeLimiter;
use std::sync::Arc;
use tower::layer::Layer;

/// A Tower layer that applies time limiting to a service.
///
/// The type parameter `T` is the timeout source type:
/// - `TimeLimiterLayer<FixedTimeout>` - uses fixed timeout (works with any request type)
/// - `TimeLimiterLayer<DynamicTimeout<F>>` - uses dynamic timeout from request
///
/// # Usage
///
/// ## Fixed Timeout (simple, no type parameters needed)
///
/// ```rust
/// use tower::{ServiceBuilder, service_fn};
/// use tower_resilience_timelimiter::TimeLimiterLayer;
/// use std::time::Duration;
///
/// // No type parameters needed!
/// let layer = TimeLimiterLayer::builder()
///     .timeout_duration(Duration::from_secs(30))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: String| async move { Ok::<_, ()>(req) }));
/// ```
///
/// ## Dynamic Timeout (per-request)
///
/// ```rust
/// use tower::{ServiceBuilder, service_fn};
/// use tower_resilience_timelimiter::TimeLimiterLayer;
/// use std::time::Duration;
///
/// #[derive(Clone)]
/// struct MyRequest { timeout_ms: Option<u64> }
///
/// // Types inferred from closure
/// let layer = TimeLimiterLayer::builder()
///     .timeout_fn(|req: &MyRequest| {
///         req.timeout_ms
///             .map(Duration::from_millis)
///             .unwrap_or(Duration::from_secs(5))
///     })
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: MyRequest| async move { Ok::<_, ()>(format!("{:?}", req.timeout_ms)) }));
/// ```
#[derive(Clone)]
pub struct TimeLimiterLayer<T = FixedTimeout> {
    config: Arc<TimeLimiterConfig<T>>,
}

impl<T> TimeLimiterLayer<T> {
    /// Creates a new time limiter layer from the given configuration.
    pub(crate) fn new(config: impl Into<Arc<TimeLimiterConfig<T>>>) -> Self {
        Self {
            config: config.into(),
        }
    }
}

impl TimeLimiterLayer<FixedTimeout> {
    /// Creates a new builder for configuring a time limiter layer.
    ///
    /// # Examples
    ///
    /// ## Fixed timeout (simple, no type parameters)
    ///
    /// ```rust
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// // No type parameters needed!
    /// let layer = TimeLimiterLayer::builder()
    ///     .timeout_duration(Duration::from_secs(30))
    ///     .cancel_running_future(true)
    ///     .build();
    /// ```
    ///
    /// ## Per-request timeout (dynamic)
    ///
    /// ```rust
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// #[derive(Clone)]
    /// struct MyRequest {
    ///     timeout_ms: Option<u64>,
    /// }
    ///
    /// // Types inferred from closure
    /// let layer = TimeLimiterLayer::builder()
    ///     .timeout_fn(|req: &MyRequest| {
    ///         req.timeout_ms
    ///             .map(Duration::from_millis)
    ///             .unwrap_or(Duration::from_secs(5))
    ///     })
    ///     .build();
    /// ```
    pub fn builder() -> crate::TimeLimiterConfigBuilder<FixedTimeout> {
        crate::TimeLimiterConfigBuilder::new()
    }
}

impl<T> From<TimeLimiterConfig<T>> for TimeLimiterLayer<T> {
    fn from(config: TimeLimiterConfig<T>) -> Self {
        Self::new(config)
    }
}

// Implement Layer<S> for FixedTimeout - works with any service
impl<S> Layer<S> for TimeLimiterLayer<FixedTimeout> {
    type Service = TimeLimiter<S, FixedTimeout>;

    fn layer(&self, service: S) -> Self::Service {
        TimeLimiter::new(service, Arc::clone(&self.config))
    }
}

// Implement Layer<S> for DynamicTimeout - the closure determines compatible services
impl<S, F> Layer<S> for TimeLimiterLayer<crate::config::DynamicTimeout<F>>
where
    F: 'static,
{
    type Service = TimeLimiter<S, crate::config::DynamicTimeout<F>>;

    fn layer(&self, service: S) -> Self::Service {
        TimeLimiter::new(service, Arc::clone(&self.config))
    }
}
