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
