//! Configuration for health checking behavior.

use crate::SelectionStrategy;
use std::time::Duration;

#[cfg(feature = "tracing")]
use crate::HealthStatus;
#[cfg(feature = "tracing")]
use std::sync::Arc;

/// Type alias for health change callback
#[cfg(feature = "tracing")]
type HealthChangeCallback = Arc<dyn Fn(&str, HealthStatus, HealthStatus) + Send + Sync>;

/// Type alias for check failed callback
#[cfg(feature = "tracing")]
type CheckFailedCallback = Arc<dyn Fn(&str, &dyn std::error::Error) + Send + Sync>;

/// Configuration for health checking behavior.
#[derive(Clone)]
pub struct HealthCheckConfig {
    /// Interval between health checks
    pub(crate) interval: Duration,

    /// Initial delay before starting health checks
    pub(crate) initial_delay: Duration,

    /// Timeout for individual health checks
    pub(crate) timeout: Duration,

    /// Number of consecutive successes to mark as healthy
    pub(crate) success_threshold: u32,

    /// Number of consecutive failures to mark as unhealthy
    pub(crate) failure_threshold: u32,

    /// Selection strategy for choosing healthy resources
    pub(crate) selection_strategy: SelectionStrategy,

    /// Event callbacks (behind tracing feature)
    #[cfg(feature = "tracing")]
    pub(crate) on_health_change: Option<HealthChangeCallback>,

    #[cfg(feature = "tracing")]
    #[allow(dead_code)]
    pub(crate) on_check_failed: Option<CheckFailedCallback>,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            initial_delay: Duration::from_millis(500),
            timeout: Duration::from_secs(2),
            success_threshold: 1,
            failure_threshold: 2,
            selection_strategy: SelectionStrategy::default(),
            #[cfg(feature = "tracing")]
            on_health_change: None,
            #[cfg(feature = "tracing")]
            on_check_failed: None,
        }
    }
}

impl HealthCheckConfig {
    /// Create a new builder.
    pub fn builder() -> HealthCheckConfigBuilder {
        HealthCheckConfigBuilder::default()
    }

    /// Get the health check interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Get the initial delay.
    pub fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    /// Get the timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Get the success threshold.
    pub fn success_threshold(&self) -> u32 {
        self.success_threshold
    }

    /// Get the failure threshold.
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }
}

/// Builder for `HealthCheckConfig`.
#[derive(Default)]
pub struct HealthCheckConfigBuilder {
    interval: Option<Duration>,
    initial_delay: Option<Duration>,
    timeout: Option<Duration>,
    success_threshold: Option<u32>,
    failure_threshold: Option<u32>,
    selection_strategy: Option<SelectionStrategy>,
    #[cfg(feature = "tracing")]
    on_health_change: Option<HealthChangeCallback>,
    #[cfg(feature = "tracing")]
    on_check_failed: Option<CheckFailedCallback>,
}

impl HealthCheckConfigBuilder {
    /// Set the interval between health checks.
    ///
    /// Default: 5 seconds
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
        self
    }

    /// Set the initial delay before starting health checks.
    ///
    /// Default: 500 milliseconds
    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = Some(delay);
        self
    }

    /// Set the timeout for individual health checks.
    ///
    /// Default: 2 seconds
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the number of consecutive successes required to mark as healthy.
    ///
    /// Default: 1
    pub fn success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = Some(threshold);
        self
    }

    /// Set the number of consecutive failures required to mark as unhealthy.
    ///
    /// Default: 2
    pub fn failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = Some(threshold);
        self
    }

    /// Set the selection strategy.
    ///
    /// Default: `SelectionStrategy::FirstAvailable`
    pub fn selection_strategy(mut self, strategy: SelectionStrategy) -> Self {
        self.selection_strategy = Some(strategy);
        self
    }

    /// Callback when health status changes.
    ///
    /// The callback receives: (resource_name, old_status, new_status)
    #[cfg(feature = "tracing")]
    pub fn on_health_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, HealthStatus, HealthStatus) + Send + Sync + 'static,
    {
        self.on_health_change = Some(Arc::new(callback));
        self
    }

    /// Callback when health check fails.
    ///
    /// The callback receives: (resource_name, error)
    #[cfg(feature = "tracing")]
    pub fn on_check_failed<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, &dyn std::error::Error) + Send + Sync + 'static,
    {
        self.on_check_failed = Some(Arc::new(callback));
        self
    }

    /// Build the configuration.
    pub fn build(self) -> HealthCheckConfig {
        let default = HealthCheckConfig::default();
        HealthCheckConfig {
            interval: self.interval.unwrap_or(default.interval),
            initial_delay: self.initial_delay.unwrap_or(default.initial_delay),
            timeout: self.timeout.unwrap_or(default.timeout),
            success_threshold: self.success_threshold.unwrap_or(default.success_threshold),
            failure_threshold: self.failure_threshold.unwrap_or(default.failure_threshold),
            selection_strategy: self
                .selection_strategy
                .unwrap_or(default.selection_strategy),
            #[cfg(feature = "tracing")]
            on_health_change: self.on_health_change,
            #[cfg(feature = "tracing")]
            on_check_failed: self.on_check_failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.interval(), Duration::from_secs(5));
        assert_eq!(config.initial_delay(), Duration::from_millis(500));
        assert_eq!(config.timeout(), Duration::from_secs(2));
        assert_eq!(config.success_threshold(), 1);
        assert_eq!(config.failure_threshold(), 2);
    }

    #[test]
    fn test_builder() {
        let config = HealthCheckConfig::builder()
            .interval(Duration::from_secs(10))
            .initial_delay(Duration::from_secs(1))
            .timeout(Duration::from_secs(5))
            .success_threshold(3)
            .failure_threshold(5)
            .build();

        assert_eq!(config.interval(), Duration::from_secs(10));
        assert_eq!(config.initial_delay(), Duration::from_secs(1));
        assert_eq!(config.timeout(), Duration::from_secs(5));
        assert_eq!(config.success_threshold(), 3);
        assert_eq!(config.failure_threshold(), 5);
    }

    #[test]
    fn test_builder_partial() {
        let config = HealthCheckConfig::builder()
            .interval(Duration::from_secs(15))
            .build();

        assert_eq!(config.interval(), Duration::from_secs(15));
        assert_eq!(config.timeout(), Duration::from_secs(2)); // Default
    }
}
