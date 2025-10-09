//! Tower layer for time limiter.

use crate::config::TimeLimiterConfig;
use crate::TimeLimiter;
use std::sync::Arc;
use tower::layer::Layer;

/// A Tower layer that applies time limiting to a service.
#[derive(Clone)]
pub struct TimeLimiterLayer {
    config: Arc<TimeLimiterConfig>,
}

impl TimeLimiterLayer {
    /// Creates a new time limiter layer from the given configuration.
    pub(crate) fn new(config: impl Into<Arc<TimeLimiterConfig>>) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Creates a new builder for configuring a time limiter layer.
    ///
    /// This is a convenience method that delegates to [`TimeLimiterLayer::builder()`](crate::TimeLimiterLayer::builder).
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let layer = TimeLimiterLayer::builder()
    ///     .timeout_duration(Duration::from_secs(30))
    ///     .cancel_running_future(true)
    ///     .build();
    /// ```
    pub fn builder() -> crate::TimeLimiterConfigBuilder {
        TimeLimiterConfig::builder()
    }
}

impl From<TimeLimiterConfig> for TimeLimiterLayer {
    fn from(config: TimeLimiterConfig) -> Self {
        Self::new(config)
    }
}

impl<S> Layer<S> for TimeLimiterLayer {
    type Service = TimeLimiter<S>;

    fn layer(&self, service: S) -> Self::Service {
        TimeLimiter::new(service, Arc::clone(&self.config))
    }
}
