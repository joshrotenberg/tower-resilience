//! Health-triggered notifications for resilience patterns.
//!
//! This module provides the integration between health checking and
//! resilience patterns like circuit breakers.

use crate::HealthStatus;
use tower_resilience_core::{SharedHealthTrigger, TriggerHealth};

impl From<HealthStatus> for TriggerHealth {
    fn from(status: HealthStatus) -> Self {
        match status {
            HealthStatus::Healthy => TriggerHealth::Healthy,
            HealthStatus::Degraded => TriggerHealth::Degraded,
            HealthStatus::Unhealthy | HealthStatus::Unknown => TriggerHealth::Unhealthy,
        }
    }
}

/// Notifies all triggers when health status changes.
///
/// Only sends notifications when the effective trigger status changes
/// (e.g., Healthy -> Unhealthy), not on every health check.
pub(crate) fn notify_triggers(
    triggers: &[SharedHealthTrigger],
    from: HealthStatus,
    to: HealthStatus,
) {
    let from_trigger = TriggerHealth::from(from);
    let to_trigger = TriggerHealth::from(to);

    // No effective change in trigger status
    if from_trigger == to_trigger {
        return;
    }

    for trigger in triggers {
        match to_trigger {
            TriggerHealth::Healthy => trigger.trigger_healthy(),
            TriggerHealth::Degraded => trigger.trigger_degraded(),
            TriggerHealth::Unhealthy => trigger.trigger_unhealthy(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tower_resilience_core::HealthTriggerable;

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
    fn test_health_status_to_trigger_health() {
        assert_eq!(
            TriggerHealth::from(HealthStatus::Healthy),
            TriggerHealth::Healthy
        );
        assert_eq!(
            TriggerHealth::from(HealthStatus::Degraded),
            TriggerHealth::Degraded
        );
        assert_eq!(
            TriggerHealth::from(HealthStatus::Unhealthy),
            TriggerHealth::Unhealthy
        );
        assert_eq!(
            TriggerHealth::from(HealthStatus::Unknown),
            TriggerHealth::Unhealthy
        );
    }

    #[test]
    fn test_notify_triggers_healthy_to_unhealthy() {
        let trigger = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger.clone()];

        notify_triggers(&triggers, HealthStatus::Healthy, HealthStatus::Unhealthy);

        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(trigger.healthy_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_notify_triggers_unhealthy_to_healthy() {
        let trigger = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger.clone()];

        notify_triggers(&triggers, HealthStatus::Unhealthy, HealthStatus::Healthy);

        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 0);
        assert_eq!(trigger.healthy_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_notify_triggers_no_change() {
        let trigger = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger.clone()];

        // Same status - no notification
        notify_triggers(&triggers, HealthStatus::Healthy, HealthStatus::Healthy);

        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 0);
        assert_eq!(trigger.healthy_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_notify_triggers_unknown_to_unhealthy() {
        let trigger = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger.clone()];

        // Unknown -> Unhealthy: both map to Unhealthy, no notification
        notify_triggers(&triggers, HealthStatus::Unknown, HealthStatus::Unhealthy);

        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_notify_multiple_triggers() {
        let trigger1 = Arc::new(MockTrigger::new());
        let trigger2 = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger1.clone(), trigger2.clone()];

        notify_triggers(&triggers, HealthStatus::Healthy, HealthStatus::Unhealthy);

        assert_eq!(trigger1.unhealthy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(trigger2.unhealthy_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_notify_triggers_degraded() {
        let trigger = Arc::new(MockTrigger::new());
        let triggers: Vec<SharedHealthTrigger> = vec![trigger.clone()];

        notify_triggers(&triggers, HealthStatus::Healthy, HealthStatus::Degraded);

        assert_eq!(trigger.degraded_calls.load(Ordering::SeqCst), 1);
        assert_eq!(trigger.healthy_calls.load(Ordering::SeqCst), 0);
        assert_eq!(trigger.unhealthy_calls.load(Ordering::SeqCst), 0);
    }
}
