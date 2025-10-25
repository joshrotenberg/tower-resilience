//! Health check wrapper for managing multiple resources.

use crate::{HealthCheckConfig, HealthCheckedContext, HealthChecker, HealthDetail, HealthStatus};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Wrapper that manages multiple resources with health checking and automatic selection.
///
/// # Examples
///
/// ```rust
/// use tower_resilience_healthcheck::{HealthCheckWrapper, HealthStatus};
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let wrapper = HealthCheckWrapper::builder()
///     .with_context("primary", "primary")
///     .with_context("secondary", "secondary")
///     .with_checker(|resource: &str| async move {
///         // Your health check logic
///         HealthStatus::Healthy
///     })
///     .with_interval(Duration::from_secs(5))
///     .build();
///
/// // Start background health checking
/// wrapper.start().await;
///
/// // Get a healthy resource
/// if let Some(resource) = wrapper.get_healthy().await {
///     println!("Using: {}", resource);
/// }
///
/// // Stop health checking
/// wrapper.stop().await;
/// # Ok(())
/// # }
/// ```
pub struct HealthCheckWrapper<T, C> {
    /// All monitored resources
    contexts: Arc<RwLock<Vec<HealthCheckedContext<T>>>>,

    /// Health checker implementation
    checker: Arc<C>,

    /// Configuration
    config: HealthCheckConfig,

    /// Handle to the background health check task
    health_check_task: Arc<RwLock<Option<JoinHandle<()>>>>,

    /// Round-robin counter for RoundRobin strategy
    round_robin_counter: Arc<AtomicUsize>,
}

impl<T, C> HealthCheckWrapper<T, C>
where
    T: Clone + Send + Sync + 'static,
    C: HealthChecker<T> + 'static,
{
    /// Create a new builder.
    pub fn builder() -> HealthCheckWrapperBuilder<T, C> {
        HealthCheckWrapperBuilder::new()
    }

    /// Start background health checking.
    ///
    /// Spawns a background task that periodically checks the health of all resources.
    pub async fn start(&self) {
        let contexts = Arc::clone(&self.contexts);
        let checker = Arc::clone(&self.checker);
        let config = self.config.clone();

        let task = tokio::spawn(async move {
            // Initial delay
            tokio::time::sleep(config.initial_delay).await;

            let mut interval = tokio::time::interval(config.interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let contexts_read = contexts.read().await;
                let mut handles = Vec::new();

                for ctx in contexts_read.iter() {
                    let ctx_clone = ctx.clone();
                    let checker_clone = Arc::clone(&checker);
                    let timeout = config.timeout;
                    let failure_threshold = config.failure_threshold;
                    let success_threshold = config.success_threshold;

                    #[cfg(feature = "tracing")]
                    let on_health_change = config.on_health_change.clone();

                    let handle = tokio::spawn(async move {
                        // Perform health check with timeout
                        let check_result =
                            tokio::time::timeout(timeout, checker_clone.check(&ctx_clone.context))
                                .await;

                        let status = match check_result {
                            Ok(status) => status,
                            Err(_) => HealthStatus::Unhealthy, // Timeout = unhealthy
                        };

                        // Update timestamp
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        ctx_clone.set_last_check(now);

                        // Get old status for event callback
                        #[cfg(feature = "tracing")]
                        let old_status = ctx_clone.status();

                        // Update consecutive counters and status based on check result
                        match status {
                            HealthStatus::Healthy => {
                                ctx_clone.record_success();
                                if ctx_clone.consecutive_successes() >= success_threshold as u64 {
                                    ctx_clone.set_status(HealthStatus::Healthy);
                                }
                            }
                            HealthStatus::Degraded => {
                                ctx_clone.record_success();
                                ctx_clone.set_status(HealthStatus::Degraded);
                            }
                            HealthStatus::Unhealthy => {
                                ctx_clone.record_failure();
                                if ctx_clone.consecutive_failures() >= failure_threshold as u64 {
                                    ctx_clone.set_status(HealthStatus::Unhealthy);
                                }
                            }
                            HealthStatus::Unknown => {
                                // Don't change status on unknown
                            }
                        }

                        // Emit health change event if status changed
                        #[cfg(feature = "tracing")]
                        {
                            let new_status = ctx_clone.status();
                            if old_status != new_status {
                                if let Some(ref callback) = on_health_change {
                                    callback(&ctx_clone.name, old_status, new_status);
                                }
                            }
                        }
                    });

                    handles.push(handle);
                }

                drop(contexts_read);

                // Wait for all checks to complete
                for handle in handles {
                    let _ = handle.await;
                }
            }
        });

        let mut task_lock = self.health_check_task.write().await;
        *task_lock = Some(task);
    }

    /// Stop background health checking.
    pub async fn stop(&self) {
        let mut task_lock = self.health_check_task.write().await;
        if let Some(task) = task_lock.take() {
            task.abort();
        }
    }

    /// Get a healthy resource based on the selection strategy.
    ///
    /// Returns `None` if no healthy resources are available.
    pub async fn get_healthy(&self) -> Option<T> {
        self.get_with_filter(|s| s == HealthStatus::Healthy).await
    }

    /// Get a usable resource (healthy or degraded).
    ///
    /// Returns `None` if no usable resources are available.
    pub async fn get_usable(&self) -> Option<T> {
        self.get_with_filter(|s| s.is_usable()).await
    }

    /// Get a resource matching the filter function.
    async fn get_with_filter<F>(&self, filter: F) -> Option<T>
    where
        F: Fn(HealthStatus) -> bool,
    {
        let contexts = self.contexts.read().await;

        // Filter to contexts matching the filter
        let available: Vec<_> = contexts
            .iter()
            .filter(|ctx| filter(ctx.status()))
            .cloned()
            .collect();

        if available.is_empty() {
            return None;
        }

        // Select based on strategy
        let selected_idx = self
            .config
            .selection_strategy
            .select(&available, &self.round_robin_counter)?;

        available.get(selected_idx).map(|ctx| ctx.context.clone())
    }

    /// Get the health status of a specific resource by name.
    pub async fn get_status(&self, name: &str) -> Option<HealthStatus> {
        let contexts = self.contexts.read().await;
        contexts
            .iter()
            .find(|ctx| ctx.name == name)
            .map(|ctx| ctx.status())
    }

    /// Get health status of all resources.
    pub async fn get_all_statuses(&self) -> Vec<(String, HealthStatus)> {
        let contexts = self.contexts.read().await;
        contexts
            .iter()
            .map(|ctx| (ctx.name.clone(), ctx.status()))
            .collect()
    }

    /// Get detailed health information for all resources.
    pub async fn get_health_details(&self) -> Vec<HealthDetail> {
        let contexts = self.contexts.read().await;
        contexts
            .iter()
            .map(|ctx| HealthDetail {
                name: ctx.name.clone(),
                status: ctx.status(),
                last_check_millis: ctx.last_check(),
                consecutive_failures: ctx.consecutive_failures(),
                consecutive_successes: ctx.consecutive_successes(),
            })
            .collect()
    }
}

impl<T, C> Drop for HealthCheckWrapper<T, C> {
    fn drop(&mut self) {
        // Abort the background task if it's still running
        if let Some(task) = self
            .health_check_task
            .try_write()
            .ok()
            .and_then(|mut guard| guard.take())
        {
            task.abort();
        }
    }
}

/// Builder for `HealthCheckWrapper`.
pub struct HealthCheckWrapperBuilder<T, C> {
    contexts: Vec<HealthCheckedContext<T>>,
    checker: Option<C>,
    config: HealthCheckConfig,
}

impl<T, C> HealthCheckWrapperBuilder<T, C>
where
    T: Clone + Send + Sync + 'static,
    C: HealthChecker<T> + 'static,
{
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            contexts: Vec::new(),
            checker: None,
            config: HealthCheckConfig::default(),
        }
    }

    /// Add a resource to monitor.
    pub fn with_context(mut self, context: T, name: impl Into<String>) -> Self {
        self.contexts.push(HealthCheckedContext::new(context, name));
        self
    }

    /// Set the health checker.
    pub fn with_checker(mut self, checker: C) -> Self {
        self.checker = Some(checker);
        self
    }

    /// Set the health check interval.
    pub fn with_interval(mut self, interval: std::time::Duration) -> Self {
        self.config.interval = interval;
        self
    }

    /// Set the initial delay before starting health checks.
    pub fn with_initial_delay(mut self, delay: std::time::Duration) -> Self {
        self.config.initial_delay = delay;
        self
    }

    /// Set the timeout for individual health checks.
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set the failure threshold.
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.config.failure_threshold = threshold;
        self
    }

    /// Set the success threshold.
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.config.success_threshold = threshold;
        self
    }

    /// Set the selection strategy.
    pub fn with_selection_strategy(mut self, strategy: crate::SelectionStrategy) -> Self {
        self.config.selection_strategy = strategy;
        self
    }

    /// Set the full configuration.
    pub fn with_config(mut self, config: HealthCheckConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the `HealthCheckWrapper`.
    ///
    /// # Panics
    ///
    /// Panics if no health checker was provided.
    pub fn build(self) -> HealthCheckWrapper<T, C> {
        HealthCheckWrapper {
            contexts: Arc::new(RwLock::new(self.contexts)),
            checker: Arc::new(self.checker.expect("Health checker must be provided")),
            config: self.config,
            health_check_task: Arc::new(RwLock::new(None)),
            round_robin_counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl<T, C> Default for HealthCheckWrapperBuilder<T, C>
where
    T: Clone + Send + Sync + 'static,
    C: HealthChecker<T> + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Clone)]
    struct MockResource {
        name: String,
        is_healthy: bool,
    }

    struct MockChecker;

    impl HealthChecker<MockResource> for MockChecker {
        async fn check(&self, resource: &MockResource) -> HealthStatus {
            if resource.is_healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unhealthy
            }
        }
    }

    #[tokio::test]
    async fn test_wrapper_builder() {
        let wrapper = HealthCheckWrapper::builder()
            .with_context(
                MockResource {
                    name: "test".to_string(),
                    is_healthy: true,
                },
                "test",
            )
            .with_checker(MockChecker)
            .with_interval(Duration::from_millis(100))
            .build();

        // Should start with unknown status
        let statuses = wrapper.get_all_statuses().await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].1, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_get_healthy_after_check() {
        let wrapper = HealthCheckWrapper::builder()
            .with_context(
                MockResource {
                    name: "healthy".to_string(),
                    is_healthy: true,
                },
                "healthy",
            )
            .with_context(
                MockResource {
                    name: "unhealthy".to_string(),
                    is_healthy: false,
                },
                "unhealthy",
            )
            .with_checker(MockChecker)
            .with_interval(Duration::from_millis(50))
            .with_initial_delay(Duration::from_millis(10))
            .build();

        wrapper.start().await;

        // Wait for health checks to run
        tokio::time::sleep(Duration::from_millis(100)).await;

        let healthy = wrapper.get_healthy().await;
        assert!(healthy.is_some());
        assert!(healthy.unwrap().is_healthy);

        wrapper.stop().await;
    }

    #[tokio::test]
    async fn test_no_healthy_resources() {
        let wrapper = HealthCheckWrapper::builder()
            .with_context(
                MockResource {
                    name: "unhealthy".to_string(),
                    is_healthy: false,
                },
                "unhealthy",
            )
            .with_checker(MockChecker)
            .with_interval(Duration::from_millis(50))
            .with_initial_delay(Duration::from_millis(10))
            .with_failure_threshold(1)
            .build();

        wrapper.start().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let healthy = wrapper.get_healthy().await;
        assert!(healthy.is_none());

        wrapper.stop().await;
    }

    #[tokio::test]
    async fn test_get_status_by_name() {
        let wrapper = HealthCheckWrapper::builder()
            .with_context(
                MockResource {
                    name: "resource1".to_string(),
                    is_healthy: true,
                },
                "resource1",
            )
            .with_checker(MockChecker)
            .with_interval(Duration::from_millis(50))
            .with_initial_delay(Duration::from_millis(10))
            .build();

        wrapper.start().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let status = wrapper.get_status("resource1").await;
        assert_eq!(status, Some(HealthStatus::Healthy));

        let missing = wrapper.get_status("missing").await;
        assert_eq!(missing, None);

        wrapper.stop().await;
    }
}
