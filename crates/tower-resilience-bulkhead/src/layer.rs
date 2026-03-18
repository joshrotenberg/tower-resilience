//! Tower layer implementation for bulkhead.

use crate::config::BulkheadConfig;
use crate::service::Bulkhead;
use std::sync::Arc;
use tokio::sync::Semaphore;
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
    pub(crate) config: BulkheadConfig,
    /// Pre-created shared state (set by `build_with_handle()`).
    pub(crate) shared: Option<(Arc<Semaphore>, Arc<BulkheadConfig>)>,
}

impl BulkheadLayer {
    /// Creates a new bulkhead layer with the given configuration.
    pub fn new(config: BulkheadConfig) -> Self {
        Self {
            config,
            shared: None,
        }
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
    ///     .max_wait_duration(Duration::from_secs(5))
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

    // =========================================================================
    // Presets
    // =========================================================================

    /// Preset: Small bulkhead for limited concurrency.
    ///
    /// Configuration:
    /// - 10 maximum concurrent calls
    /// - No wait timeout (rejects immediately when full)
    ///
    /// Use this for protecting resources with limited capacity, such as
    /// database connection pools or external API rate limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_bulkhead::BulkheadLayer;
    ///
    /// let layer = BulkheadLayer::small().build();
    ///
    /// // Or customize further
    /// let layer = BulkheadLayer::small()
    ///     .max_wait_duration(std::time::Duration::from_secs(5))
    ///     .build();
    /// ```
    pub fn small() -> crate::BulkheadConfigBuilder {
        Self::builder().max_concurrent_calls(10).reject_when_full()
    }

    /// Preset: Medium bulkhead for moderate concurrency.
    ///
    /// Configuration:
    /// - 50 maximum concurrent calls
    /// - No wait timeout (rejects immediately when full)
    ///
    /// A balanced configuration for typical service-to-service communication.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_bulkhead::BulkheadLayer;
    ///
    /// let layer = BulkheadLayer::medium().build();
    /// ```
    pub fn medium() -> crate::BulkheadConfigBuilder {
        Self::builder().max_concurrent_calls(50).reject_when_full()
    }

    /// Preset: Large bulkhead for high concurrency.
    ///
    /// Configuration:
    /// - 200 maximum concurrent calls
    /// - No wait timeout (rejects immediately when full)
    ///
    /// Use this for high-throughput services that can handle many
    /// concurrent requests.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_bulkhead::BulkheadLayer;
    ///
    /// let layer = BulkheadLayer::large().build();
    /// ```
    pub fn large() -> crate::BulkheadConfigBuilder {
        Self::builder().max_concurrent_calls(200).reject_when_full()
    }
}

impl<S> Layer<S> for BulkheadLayer {
    type Service = Bulkhead<S>;

    fn layer(&self, service: S) -> Self::Service {
        if let Some((semaphore, config)) = &self.shared {
            Bulkhead::from_shared(service, Arc::clone(semaphore), Arc::clone(config))
        } else {
            Bulkhead::new(service, self.config.clone())
        }
    }
}
