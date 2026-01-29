//! Health-based triggering for resilience patterns.
//!
//! This module provides a trait-based integration that allows health monitoring
//! systems to proactively control resilience patterns like circuit breakers.
//!
//! # Example
//!
//! ```rust,ignore
//! use tower_resilience_core::HealthTriggerable;
//! use std::sync::Arc;
//!
//! // When health check detects an unhealthy resource
//! fn on_unhealthy(trigger: &dyn HealthTriggerable) {
//!     trigger.trigger_unhealthy();
//! }
//! ```

use std::sync::Arc;

/// Health status for triggering purposes.
///
/// This is separate from any crate-specific health status to avoid
/// coupling between resilience patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerHealth {
    /// Resource is healthy and ready for use.
    Healthy,

    /// Resource is degraded but still functional.
    Degraded,

    /// Resource is unhealthy and should not be used.
    Unhealthy,
}

/// Trait for components that can be triggered by health changes.
///
/// Implementations must be non-blocking. For async operations,
/// spawn tasks internally rather than blocking the caller.
///
/// # Thread Safety
///
/// This trait requires `Send + Sync` because health checks typically
/// run in background tasks and need to trigger multiple components.
///
/// # Example Implementation
///
/// ```rust,ignore
/// use tower_resilience_core::HealthTriggerable;
///
/// struct MyCircuitBreaker {
///     // ...
/// }
///
/// impl HealthTriggerable for MyCircuitBreaker {
///     fn trigger_unhealthy(&self) {
///         // Open the circuit immediately
///         // Use tokio::spawn for async operations
///     }
///
///     fn trigger_healthy(&self) {
///         // Close the circuit
///     }
/// }
/// ```
pub trait HealthTriggerable: Send + Sync {
    /// Called when health becomes unhealthy.
    ///
    /// This should cause the component to enter a protective state
    /// (e.g., circuit breaker opens).
    fn trigger_unhealthy(&self);

    /// Called when health becomes healthy.
    ///
    /// This should cause the component to resume normal operation
    /// (e.g., circuit breaker closes).
    fn trigger_healthy(&self);

    /// Called when health becomes degraded.
    ///
    /// Default implementation is a no-op since degraded services
    /// are typically still usable. Override if your component needs
    /// special handling for degraded state.
    fn trigger_degraded(&self) {}
}

/// Shared reference to a health trigger.
///
/// Use this type when storing triggers in collections or passing
/// them between components.
pub type SharedHealthTrigger = Arc<dyn HealthTriggerable>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockTrigger {
        unhealthy_calls: AtomicU32,
        healthy_calls: AtomicU32,
        degraded_calls: AtomicU32,
    }

    impl MockTrigger {
        fn new() -> Self {
            Self {
                unhealthy_calls: AtomicU32::new(0),
                healthy_calls: AtomicU32::new(0),
                degraded_calls: AtomicU32::new(0),
            }
        }
    }

    impl HealthTriggerable for MockTrigger {
        fn trigger_unhealthy(&self) {
            self.unhealthy_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn trigger_healthy(&self) {
            self.healthy_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn trigger_degraded(&self) {
            self.degraded_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_trigger_methods() {
        let trigger = MockTrigger::new();

        trigger.trigger_unhealthy();
        trigger.trigger_healthy();
        trigger.trigger_degraded();

        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(trigger.healthy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(trigger.degraded_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_shared_trigger() {
        let trigger: SharedHealthTrigger = Arc::new(MockTrigger::new());

        trigger.trigger_unhealthy();
        trigger.trigger_healthy();

        // Can clone and use from multiple places
        let trigger2 = Arc::clone(&trigger);
        trigger2.trigger_unhealthy();
    }

    #[test]
    fn test_trigger_health_equality() {
        assert_eq!(TriggerHealth::Healthy, TriggerHealth::Healthy);
        assert_ne!(TriggerHealth::Healthy, TriggerHealth::Unhealthy);
        assert_ne!(TriggerHealth::Degraded, TriggerHealth::Unhealthy);
    }
}
