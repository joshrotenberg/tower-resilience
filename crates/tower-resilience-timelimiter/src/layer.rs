//! Tower layer for time limiter.

use crate::config::TimeLimiterConfig;
use crate::TimeLimiter;
use std::marker::PhantomData;
use std::sync::Arc;
use tower::layer::Layer;

/// A Tower layer that applies time limiting to a service.
#[derive(Clone)]
pub struct TimeLimiterLayer<Req> {
    config: Arc<TimeLimiterConfig<Req>>,
}

impl<Req> TimeLimiterLayer<Req> {
    /// Creates a new time limiter layer from the given configuration.
    pub(crate) fn new(config: impl Into<Arc<TimeLimiterConfig<Req>>>) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Creates a new builder for configuring a time limiter layer.
    ///
    /// # Examples
    ///
    /// ## Fixed timeout (simple)
    ///
    /// ```rust
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let layer = TimeLimiterLayer::<()>::builder()
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
    /// let layer = TimeLimiterLayer::<MyRequest>::builder()
    ///     .timeout_fn(|req: &MyRequest| {
    ///         req.timeout_ms
    ///             .map(Duration::from_millis)
    ///             .unwrap_or(Duration::from_secs(5))
    ///     })
    ///     .build();
    /// ```
    pub fn builder() -> crate::TimeLimiterConfigBuilder<Req> {
        crate::TimeLimiterConfigBuilder::new()
    }
}

impl<Req> From<TimeLimiterConfig<Req>> for TimeLimiterLayer<Req> {
    fn from(config: TimeLimiterConfig<Req>) -> Self {
        Self::new(config)
    }
}

impl<S, Req> Layer<S> for TimeLimiterLayer<Req>
where
    Req: 'static,
{
    type Service = TimeLimiter<S, Req>;

    fn layer(&self, service: S) -> Self::Service {
        TimeLimiter::new(service, Arc::clone(&self.config), PhantomData)
    }
}
