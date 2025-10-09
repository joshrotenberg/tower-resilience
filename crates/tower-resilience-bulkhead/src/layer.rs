//! Tower layer implementation for bulkhead.

use crate::config::BulkheadConfig;
use crate::service::Bulkhead;
use tower::Layer;

#[cfg(feature = "metrics")]
use metrics::{describe_counter, describe_gauge, describe_histogram};
#[cfg(feature = "metrics")]
use std::sync::Once;

#[cfg(feature = "metrics")]
static METRICS_INIT: Once = Once::new();

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
        #[cfg(feature = "metrics")]
        {
            METRICS_INIT.call_once(|| {
                describe_counter!(
                    "bulkhead_calls_permitted_total",
                    "Total number of calls permitted through the bulkhead"
                );
                describe_counter!(
                    "bulkhead_calls_rejected_total",
                    "Total number of calls rejected by the bulkhead"
                );
                describe_counter!(
                    "bulkhead_calls_finished_total",
                    "Total number of calls that finished successfully"
                );
                describe_counter!(
                    "bulkhead_calls_failed_total",
                    "Total number of calls that failed"
                );
                describe_gauge!(
                    "bulkhead_concurrent_calls",
                    "Current number of concurrent calls"
                );
                describe_histogram!(
                    "bulkhead_wait_duration_seconds",
                    "Time spent waiting to acquire a permit"
                );
                describe_histogram!(
                    "bulkhead_call_duration_seconds",
                    "Duration of calls through the bulkhead"
                );
            });
        }
        crate::BulkheadConfigBuilder::new()
    }
}

impl<S> Layer<S> for BulkheadLayer {
    type Service = Bulkhead<S>;

    fn layer(&self, service: S) -> Self::Service {
        Bulkhead::new(service, self.config.clone())
    }
}
