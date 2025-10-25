//! Health-checked context for wrapping resources.

use crate::HealthStatus;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A resource with health tracking and custom metrics.
#[derive(Debug)]
pub struct HealthCheckedContext<T> {
    /// The actual resource being monitored
    pub context: T,

    /// Human-readable name for this resource
    pub name: String,

    /// Health check state (protected by RwLock for concurrent access)
    state: Arc<RwLock<ContextState>>,

    /// Extension storage for custom metrics
    extensions: Arc<RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>>,
}

#[derive(Debug)]
struct ContextState {
    status: HealthStatus,
    last_check_millis: u64,
    consecutive_failures: u64,
    consecutive_successes: u64,
}

impl<T> HealthCheckedContext<T> {
    /// Create a new health-checked context.
    pub fn new(context: T, name: impl Into<String>) -> Self {
        Self {
            context,
            name: name.into(),
            state: Arc::new(RwLock::new(ContextState {
                status: HealthStatus::Unknown,
                last_check_millis: 0,
                consecutive_failures: 0,
                consecutive_successes: 0,
            })),
            extensions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the current health status.
    pub fn status(&self) -> HealthStatus {
        self.state.read().unwrap().status
    }

    /// Update the health status.
    pub(crate) fn set_status(&self, status: HealthStatus) {
        self.state.write().unwrap().status = status;
    }

    /// Get the timestamp of the last health check (milliseconds since epoch).
    pub fn last_check(&self) -> u64 {
        self.state.read().unwrap().last_check_millis
    }

    /// Update the last check timestamp.
    pub(crate) fn set_last_check(&self, timestamp: u64) {
        self.state.write().unwrap().last_check_millis = timestamp;
    }

    /// Get consecutive failure count.
    pub fn consecutive_failures(&self) -> u64 {
        self.state.read().unwrap().consecutive_failures
    }

    /// Get consecutive success count.
    pub fn consecutive_successes(&self) -> u64 {
        self.state.read().unwrap().consecutive_successes
    }

    /// Increment consecutive failures, reset successes.
    pub(crate) fn record_failure(&self) {
        let mut state = self.state.write().unwrap();
        state.consecutive_failures += 1;
        state.consecutive_successes = 0;
    }

    /// Increment consecutive successes, reset failures.
    pub(crate) fn record_success(&self) {
        let mut state = self.state.write().unwrap();
        state.consecutive_successes += 1;
        state.consecutive_failures = 0;
    }

    /// Store a custom metric.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use tower_resilience_healthcheck::HealthCheckedContext;
    /// let ctx = HealthCheckedContext::new("resource", "test");
    /// ctx.set_extension("latency_ms", Box::new(42u64));
    /// ```
    pub fn set_extension(&self, key: impl Into<String>, value: Box<dyn Any + Send + Sync>) {
        self.extensions.write().unwrap().insert(key.into(), value);
    }

    /// Get a custom metric.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use tower_resilience_healthcheck::HealthCheckedContext;
    /// let ctx = HealthCheckedContext::new("resource", "test");
    /// ctx.set_extension("latency_ms", Box::new(42u64));
    ///
    /// if let Some(latency) = ctx.get_extension::<u64>("latency_ms") {
    ///     println!("Latency: {}ms", latency);
    /// }
    /// ```
    pub fn get_extension<V>(&self, key: &str) -> Option<V>
    where
        V: Any + Send + Sync + Clone,
    {
        self.extensions
            .read()
            .unwrap()
            .get(key)
            .and_then(|v| v.downcast_ref::<V>())
            .cloned()
    }
}

impl<T: Clone> Clone for HealthCheckedContext<T> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            name: self.name.clone(),
            state: Arc::clone(&self.state),
            extensions: Arc::clone(&self.extensions),
        }
    }
}

/// Detailed health information for a resource.
#[derive(Debug, Clone)]
pub struct HealthDetail {
    /// Name of the resource
    pub name: String,

    /// Current health status
    pub status: HealthStatus,

    /// Timestamp of last check (milliseconds since epoch)
    pub last_check_millis: u64,

    /// Number of consecutive failures
    pub consecutive_failures: u64,

    /// Number of consecutive successes
    pub consecutive_successes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context() {
        let ctx = HealthCheckedContext::new("resource", "test");
        assert_eq!(ctx.name, "test");
        assert_eq!(ctx.status(), HealthStatus::Unknown);
        assert_eq!(ctx.consecutive_failures(), 0);
        assert_eq!(ctx.consecutive_successes(), 0);
    }

    #[test]
    fn test_status_updates() {
        let ctx = HealthCheckedContext::new("resource", "test");
        ctx.set_status(HealthStatus::Healthy);
        assert_eq!(ctx.status(), HealthStatus::Healthy);

        ctx.set_status(HealthStatus::Unhealthy);
        assert_eq!(ctx.status(), HealthStatus::Unhealthy);
    }

    #[test]
    fn test_record_failure() {
        let ctx = HealthCheckedContext::new("resource", "test");
        ctx.record_success();
        assert_eq!(ctx.consecutive_successes(), 1);

        ctx.record_failure();
        assert_eq!(ctx.consecutive_failures(), 1);
        assert_eq!(ctx.consecutive_successes(), 0);

        ctx.record_failure();
        assert_eq!(ctx.consecutive_failures(), 2);
    }

    #[test]
    fn test_record_success() {
        let ctx = HealthCheckedContext::new("resource", "test");
        ctx.record_failure();
        assert_eq!(ctx.consecutive_failures(), 1);

        ctx.record_success();
        assert_eq!(ctx.consecutive_successes(), 1);
        assert_eq!(ctx.consecutive_failures(), 0);

        ctx.record_success();
        assert_eq!(ctx.consecutive_successes(), 2);
    }

    #[test]
    fn test_extensions() {
        let ctx = HealthCheckedContext::new("resource", "test");
        ctx.set_extension("latency_ms", Box::new(42u64));

        let latency: Option<u64> = ctx.get_extension("latency_ms");
        assert_eq!(latency, Some(42));

        let missing: Option<u64> = ctx.get_extension("missing");
        assert_eq!(missing, None);
    }

    #[test]
    fn test_clone() {
        let ctx = HealthCheckedContext::new("resource".to_string(), "test");
        ctx.set_status(HealthStatus::Healthy);
        ctx.set_extension("key", Box::new(123u64));

        let cloned = ctx.clone();
        assert_eq!(cloned.name, ctx.name);
        assert_eq!(cloned.status(), ctx.status());

        // Should share state
        ctx.set_status(HealthStatus::Degraded);
        assert_eq!(cloned.status(), HealthStatus::Degraded);
    }
}
