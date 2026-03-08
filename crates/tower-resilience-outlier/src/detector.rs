//! Shared fleet-level outlier detector state.
//!
//! The [`OutlierDetector`] coordinates ejection state across all instances
//! in a fleet, enforcing the `max_ejection_percent` limit to prevent
//! cascading ejections.

use crate::events::OutlierDetectionEvent;
use crate::strategy::EjectionStrategy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_resilience_core::events::EventListeners;

/// State for a single instance tracked by the detector.
struct InstanceState {
    /// Whether this instance is currently ejected.
    ejected: bool,
    /// When the instance was ejected (if ejected).
    ejected_at: Option<Instant>,
    /// How long the current ejection lasts.
    ejection_duration: Duration,
    /// Number of times this instance has been ejected (for exponential backoff).
    ejection_count: usize,
    /// The ejection strategy for this instance.
    strategy: Arc<dyn EjectionStrategy>,
}

/// Shared fleet-level state for outlier detection.
///
/// The `OutlierDetector` is shared (via `Arc<Mutex<...>>` internally) across
/// all instances in a fleet. It tracks which instances are ejected and enforces
/// the `max_ejection_percent` limit.
///
/// # Examples
///
/// ```
/// use tower_resilience_outlier::OutlierDetector;
/// use std::time::Duration;
///
/// let detector = OutlierDetector::new()
///     .max_ejection_percent(50)
///     .base_ejection_duration(Duration::from_secs(30));
///
/// // Register instances
/// detector.register("backend-1", 5);
/// detector.register("backend-2", 5);
/// ```
#[derive(Clone)]
pub struct OutlierDetector {
    inner: Arc<Mutex<DetectorInner>>,
}

struct DetectorInner {
    instances: HashMap<String, InstanceState>,
    max_ejection_percent: f64,
    base_ejection_duration: Duration,
    max_ejection_duration: Option<Duration>,
    pattern_name: String,
    event_listeners: EventListeners<OutlierDetectionEvent>,
}

impl OutlierDetector {
    /// Creates a new `OutlierDetector` with default settings.
    ///
    /// Defaults:
    /// - `max_ejection_percent`: 50%
    /// - `base_ejection_duration`: 30 seconds
    /// - No max ejection duration cap
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DetectorInner {
                instances: HashMap::new(),
                max_ejection_percent: 50.0,
                base_ejection_duration: Duration::from_secs(30),
                max_ejection_duration: None,
                pattern_name: "outlier_detection".to_string(),
                event_listeners: EventListeners::new(),
            })),
        }
    }

    /// Sets the maximum percentage of instances that can be ejected simultaneously.
    ///
    /// This prevents cascading ejections from taking down the entire fleet.
    /// Value should be between 0 and 100. Default is 50.
    pub fn max_ejection_percent(self, percent: usize) -> Self {
        self.inner.lock().unwrap().max_ejection_percent = percent as f64;
        self
    }

    /// Sets the base ejection duration.
    ///
    /// The actual ejection duration for an instance is
    /// `base_ejection_duration * 2^(ejection_count - 1)`, providing
    /// exponential backoff for repeatedly-ejected instances.
    pub fn base_ejection_duration(self, duration: Duration) -> Self {
        self.inner.lock().unwrap().base_ejection_duration = duration;
        self
    }

    /// Sets the maximum ejection duration cap.
    ///
    /// Without this, exponential backoff can grow very large for
    /// repeatedly-ejected instances. When set, the ejection duration
    /// is capped at this value.
    pub fn max_ejection_duration(self, duration: Duration) -> Self {
        self.inner.lock().unwrap().max_ejection_duration = Some(duration);
        self
    }

    /// Sets the pattern name used in events.
    pub fn name(self, name: impl Into<String>) -> Self {
        self.inner.lock().unwrap().pattern_name = name.into();
        self
    }

    /// Adds an event listener for ejection events.
    pub fn on_ejection<F>(self, f: F) -> Self
    where
        F: Fn(&str, usize) + Send + Sync + 'static,
    {
        self.inner.lock().unwrap().event_listeners.add(
            tower_resilience_core::events::FnListener::new(move |event| {
                if let OutlierDetectionEvent::Ejected {
                    instance_name,
                    consecutive_errors,
                    ..
                } = event
                {
                    f(instance_name, *consecutive_errors);
                }
            }),
        );
        self
    }

    /// Adds an event listener for recovery events.
    pub fn on_recovery<F>(self, f: F) -> Self
    where
        F: Fn(&str, Duration) + Send + Sync + 'static,
    {
        self.inner.lock().unwrap().event_listeners.add(
            tower_resilience_core::events::FnListener::new(move |event| {
                if let OutlierDetectionEvent::Recovered {
                    instance_name,
                    ejected_duration,
                    ..
                } = event
                {
                    f(instance_name, *ejected_duration);
                }
            }),
        );
        self
    }

    /// Registers an instance with the detector using the consecutive errors strategy.
    ///
    /// `consecutive_error_threshold` is the number of consecutive errors
    /// before the instance is ejected.
    pub fn register(&self, name: impl Into<String>, consecutive_error_threshold: usize) {
        self.register_with_strategy(
            name,
            Arc::new(crate::strategy::ConsecutiveErrors::new(
                consecutive_error_threshold,
            )),
        );
    }

    /// Registers an instance with a custom ejection strategy.
    pub fn register_with_strategy(
        &self,
        name: impl Into<String>,
        strategy: Arc<dyn EjectionStrategy>,
    ) {
        let name = name.into();
        let mut inner = self.inner.lock().unwrap();
        inner.instances.insert(
            name,
            InstanceState {
                ejected: false,
                ejected_at: None,
                ejection_duration: Duration::ZERO,
                ejection_count: 0,
                strategy,
            },
        );
    }

    /// Records a successful call for the named instance.
    pub fn record_success(&self, name: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(instance) = inner.instances.get_mut(name) {
            instance.strategy.record_success();
        }
    }

    /// Records a failed call for the named instance.
    ///
    /// Returns `true` if the instance was ejected as a result.
    pub fn record_failure(&self, name: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();

        let (should_eject, failure_count) = {
            if let Some(instance) = inner.instances.get(name) {
                if instance.ejected {
                    return false;
                }
                let should_eject = instance.strategy.record_failure();
                let count = instance.strategy.failure_count();
                (should_eject, count)
            } else {
                return false;
            }
        };

        if !should_eject {
            return false;
        }

        // Check max_ejection_percent
        let total = inner.instances.len();
        let currently_ejected = inner.instances.values().filter(|i| i.ejected).count();
        let ejection_percent = if total > 0 {
            ((currently_ejected + 1) as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        if ejection_percent > inner.max_ejection_percent {
            let event = OutlierDetectionEvent::EjectionSkipped {
                pattern_name: inner.pattern_name.clone(),
                timestamp: Instant::now(),
                instance_name: name.to_string(),
                current_ejection_percent: (currently_ejected as f64 / total as f64) * 100.0,
            };
            inner.event_listeners.emit(&event);
            return false;
        }

        // Read config values before mutable borrow of instances
        let base_duration = inner.base_ejection_duration;
        let max_duration = inner.max_ejection_duration;
        let pattern_name = inner.pattern_name.clone();

        // Eject the instance
        let instance = inner.instances.get_mut(name).unwrap();
        instance.ejection_count += 1;
        let ejection_duration =
            Self::compute_ejection_duration(base_duration, max_duration, instance.ejection_count);
        instance.ejected = true;
        instance.ejected_at = Some(Instant::now());
        instance.ejection_duration = ejection_duration;

        let event = OutlierDetectionEvent::Ejected {
            pattern_name,
            timestamp: Instant::now(),
            instance_name: name.to_string(),
            consecutive_errors: failure_count,
            ejection_duration,
        };
        inner.event_listeners.emit(&event);

        true
    }

    /// Checks if the named instance is currently ejected.
    ///
    /// Also handles automatic recovery: if the ejection duration has elapsed,
    /// the instance is recovered and this returns `false`.
    pub fn is_ejected(&self, name: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();

        let should_recover = if let Some(instance) = inner.instances.get(name) {
            if !instance.ejected {
                return false;
            }
            if let Some(ejected_at) = instance.ejected_at {
                ejected_at.elapsed() >= instance.ejection_duration
            } else {
                false
            }
        } else {
            return false;
        };

        if should_recover {
            let instance = inner.instances.get_mut(name).unwrap();
            let ejected_duration = instance.ejected_at.map(|t| t.elapsed()).unwrap_or_default();
            instance.ejected = false;
            instance.ejected_at = None;
            instance.strategy.reset();

            let event = OutlierDetectionEvent::Recovered {
                pattern_name: inner.pattern_name.clone(),
                timestamp: Instant::now(),
                instance_name: name.to_string(),
                ejected_duration,
            };
            inner.event_listeners.emit(&event);

            return false;
        }

        true
    }

    /// Returns the number of currently ejected instances.
    pub fn ejected_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .instances
            .values()
            .filter(|i| i.ejected)
            .count()
    }

    /// Returns the total number of registered instances.
    pub fn instance_count(&self) -> usize {
        self.inner.lock().unwrap().instances.len()
    }

    /// Returns the pattern name.
    pub fn pattern_name(&self) -> String {
        self.inner.lock().unwrap().pattern_name.clone()
    }

    fn compute_ejection_duration(
        base: Duration,
        max: Option<Duration>,
        ejection_count: usize,
    ) -> Duration {
        let multiplier = 2u32.saturating_pow((ejection_count - 1) as u32);
        let duration = base.saturating_mul(multiplier);
        match max {
            Some(max_dur) => duration.min(max_dur),
            None => duration,
        }
    }
}

impl Default for OutlierDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_eject() {
        let detector = OutlierDetector::new().max_ejection_percent(100);
        detector.register("backend-1", 3);

        assert!(!detector.is_ejected("backend-1"));

        // 2 failures - not ejected yet
        assert!(!detector.record_failure("backend-1"));
        assert!(!detector.record_failure("backend-1"));

        // 3rd failure - ejected
        assert!(detector.record_failure("backend-1"));
        assert!(detector.is_ejected("backend-1"));
    }

    #[test]
    fn test_success_resets_count() {
        let detector = OutlierDetector::new().max_ejection_percent(100);
        detector.register("backend-1", 3);

        assert!(!detector.record_failure("backend-1"));
        assert!(!detector.record_failure("backend-1"));
        detector.record_success("backend-1");

        // Need 3 more consecutive failures
        assert!(!detector.record_failure("backend-1"));
        assert!(!detector.record_failure("backend-1"));
        assert!(detector.record_failure("backend-1"));
    }

    #[test]
    fn test_max_ejection_percent() {
        let detector = OutlierDetector::new().max_ejection_percent(50);

        detector.register("backend-1", 1);
        detector.register("backend-2", 1);
        detector.register("backend-3", 1);
        detector.register("backend-4", 1);

        // Eject first two (50%)
        assert!(detector.record_failure("backend-1"));
        assert!(detector.record_failure("backend-2"));

        // Third ejection would exceed 50%, should be skipped
        assert!(!detector.record_failure("backend-3"));
        assert!(!detector.is_ejected("backend-3"));

        assert_eq!(detector.ejected_count(), 2);
    }

    #[test]
    fn test_auto_recovery() {
        let detector = OutlierDetector::new()
            .base_ejection_duration(Duration::from_millis(10))
            .max_ejection_percent(100);

        detector.register("backend-1", 1);
        assert!(detector.record_failure("backend-1"));
        assert!(detector.is_ejected("backend-1"));

        // Wait for ejection to expire
        std::thread::sleep(Duration::from_millis(20));

        // Should auto-recover
        assert!(!detector.is_ejected("backend-1"));
    }

    #[test]
    fn test_exponential_backoff() {
        let dur = OutlierDetector::compute_ejection_duration(
            Duration::from_secs(30),
            None,
            1, // first ejection
        );
        assert_eq!(dur, Duration::from_secs(30));

        let dur = OutlierDetector::compute_ejection_duration(
            Duration::from_secs(30),
            None,
            2, // second ejection
        );
        assert_eq!(dur, Duration::from_secs(60));

        let dur = OutlierDetector::compute_ejection_duration(
            Duration::from_secs(30),
            None,
            3, // third ejection
        );
        assert_eq!(dur, Duration::from_secs(120));
    }

    #[test]
    fn test_max_ejection_duration_cap() {
        let dur = OutlierDetector::compute_ejection_duration(
            Duration::from_secs(30),
            Some(Duration::from_secs(90)),
            3, // would be 120s, capped at 90s
        );
        assert_eq!(dur, Duration::from_secs(90));
    }

    #[test]
    fn test_unknown_instance() {
        let detector = OutlierDetector::new();
        assert!(!detector.is_ejected("unknown"));
        assert!(!detector.record_failure("unknown"));
    }
}
