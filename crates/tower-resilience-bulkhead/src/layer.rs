//! Tower layer implementation for bulkhead.

use crate::config::BulkheadConfig;
use crate::service::Bulkhead;
use tower::Layer;

/// Layer that applies bulkhead concurrency limiting.
#[derive(Clone)]
pub struct BulkheadLayer {
    config: BulkheadConfig,
}

impl BulkheadLayer {
    /// Creates a new bulkhead layer with the given configuration.
    pub fn new(config: BulkheadConfig) -> Self {
        Self { config }
    }

    /// Creates a new builder for configuring a bulkhead layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_bulkhead::BulkheadLayer;
    /// use std::time::Duration;
    ///
    /// let layer = BulkheadLayer::builder()
    ///     .max_concurrent_calls(10)
    ///     .max_wait_duration(Some(Duration::from_secs(5)))
    ///     .build();
    /// ```
    pub fn builder() -> crate::BulkheadConfigBuilder {
        crate::BulkheadConfigBuilder::new()
    }
}

impl<S> Layer<S> for BulkheadLayer {
    type Service = Bulkhead<S>;

    fn layer(&self, service: S) -> Self::Service {
        Bulkhead::new(service, self.config.clone())
    }
}
