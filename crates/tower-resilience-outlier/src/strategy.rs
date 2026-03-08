//! Ejection strategies for outlier detection.
//!
//! Strategies determine when an instance should be ejected based on
//! observed failures. The [`EjectionStrategy`] trait defines the interface,
//! and [`ConsecutiveErrors`] provides the simplest and most common strategy.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Trait for ejection decision strategies.
///
/// Implementations track per-instance failure patterns and decide when
/// an instance should be ejected. Recovery is handled separately by the
/// [`OutlierDetector`](crate::detector::OutlierDetector) via timers.
pub trait EjectionStrategy: Send + Sync {
    /// Record a successful call. Resets failure tracking.
    fn record_success(&self);

    /// Record a failed call. Returns `true` if the instance should be ejected.
    fn record_failure(&self) -> bool;

    /// Reset the strategy state (called after successful recovery probe).
    fn reset(&self);

    /// Returns the current failure count for observability.
    fn failure_count(&self) -> usize;
}

/// Ejects an instance after N consecutive errors.
///
/// The simplest and most universally useful strategy. When an instance
/// produces N errors in a row without any successes, it is marked for
/// ejection. A single success resets the counter.
///
/// # Examples
///
/// ```
/// use tower_resilience_outlier::strategy::{ConsecutiveErrors, EjectionStrategy};
///
/// let strategy = ConsecutiveErrors::new(3);
///
/// // Two failures - not yet ejected
/// assert!(!strategy.record_failure());
/// assert!(!strategy.record_failure());
///
/// // Third failure triggers ejection
/// assert!(strategy.record_failure());
///
/// // A success resets the counter
/// strategy.record_success();
/// assert!(!strategy.record_failure());
/// ```
pub struct ConsecutiveErrors {
    threshold: usize,
    count: AtomicUsize,
}

impl ConsecutiveErrors {
    /// Creates a new `ConsecutiveErrors` strategy with the given threshold.
    ///
    /// The instance will be ejected after `threshold` consecutive errors.
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            count: AtomicUsize::new(0),
        }
    }

    /// Returns the configured threshold.
    pub fn threshold(&self) -> usize {
        self.threshold
    }

    /// Returns the current consecutive error count.
    pub fn current_count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl EjectionStrategy for ConsecutiveErrors {
    fn record_success(&self) {
        self.count.store(0, Ordering::Relaxed);
    }

    fn record_failure(&self) -> bool {
        let prev = self.count.fetch_add(1, Ordering::Relaxed);
        prev + 1 >= self.threshold
    }

    fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
    }

    fn failure_count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_errors_threshold() {
        let strategy = ConsecutiveErrors::new(3);

        assert!(!strategy.record_failure());
        assert!(!strategy.record_failure());
        assert!(strategy.record_failure());
        // Continues returning true after threshold
        assert!(strategy.record_failure());
    }

    #[test]
    fn consecutive_errors_reset_on_success() {
        let strategy = ConsecutiveErrors::new(3);

        assert!(!strategy.record_failure());
        assert!(!strategy.record_failure());
        strategy.record_success();
        assert_eq!(strategy.current_count(), 0);

        // Need 3 more consecutive failures
        assert!(!strategy.record_failure());
        assert!(!strategy.record_failure());
        assert!(strategy.record_failure());
    }

    #[test]
    fn consecutive_errors_reset() {
        let strategy = ConsecutiveErrors::new(2);

        assert!(!strategy.record_failure());
        assert!(strategy.record_failure());

        strategy.reset();
        assert_eq!(strategy.current_count(), 0);
        assert!(!strategy.record_failure());
    }

    #[test]
    fn consecutive_errors_threshold_of_one() {
        let strategy = ConsecutiveErrors::new(1);
        assert!(strategy.record_failure());
    }
}
