//! AIMD (Additive Increase Multiplicative Decrease) controller.
//!
//! This module provides a generalized AIMD controller that can be used for:
//! - Retry budgets
//! - Adaptive concurrency limits
//! - Rate limiting with backoff
//! - Any scenario requiring congestion control
//!
//! # Algorithm
//!
//! AIMD is a feedback control algorithm used in TCP congestion control:
//! - **Additive Increase**: On success, increase the limit linearly
//! - **Multiplicative Decrease**: On failure/congestion, decrease the limit by a factor
//!
//! This creates a "sawtooth" pattern that probes for available capacity while
//! quickly backing off when congestion is detected.
//!
//! # Example
//!
//! ```rust
//! use tower_resilience_core::aimd::{AimdController, AimdConfig};
//!
//! let config = AimdConfig::default()
//!     .with_initial_limit(10)
//!     .with_min_limit(1)
//!     .with_max_limit(100)
//!     .with_increase_by(1)
//!     .with_decrease_factor(0.5);
//!
//! let controller = AimdController::new(config)?;
//!
//! // On success, limit increases
//! controller.record_success();
//! assert_eq!(controller.limit(), 11);
//!
//! // On failure, limit decreases by factor
//! controller.record_failure();
//! assert_eq!(controller.limit(), 5); // 11 * 0.5 = 5.5 -> 5
//! # Ok::<(), tower_resilience_core::aimd::AimdConfigError>(())
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};

/// Configuration for an AIMD controller.
#[derive(Debug, Clone)]
pub struct AimdConfig {
    /// Initial limit value.
    pub initial_limit: usize,
    /// Minimum limit (floor).
    pub min_limit: usize,
    /// Maximum limit (ceiling).
    pub max_limit: usize,
    /// Amount to add on success (additive increase).
    pub increase_by: usize,
    /// Factor to multiply by on failure (multiplicative decrease).
    /// Should be between 0.0 and 1.0.
    pub decrease_factor: f64,
}

impl Default for AimdConfig {
    fn default() -> Self {
        Self {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 100,
            increase_by: 1,
            decrease_factor: 0.5,
        }
    }
}

impl AimdConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initial limit.
    pub fn with_initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    /// Set the minimum limit (floor).
    pub fn with_min_limit(mut self, limit: usize) -> Self {
        self.min_limit = limit;
        self
    }

    /// Set the maximum limit (ceiling).
    pub fn with_max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    /// Set the additive increase amount.
    pub fn with_increase_by(mut self, amount: usize) -> Self {
        self.increase_by = amount;
        self
    }

    /// Set the multiplicative decrease factor.
    ///
    /// Should be between 0.0 and 1.0. For example, 0.5 means
    /// the limit is halved on failure.
    pub fn with_decrease_factor(mut self, factor: f64) -> Self {
        self.decrease_factor = factor;
        self
    }

    /// Validate this configuration.
    ///
    /// Checks, in order:
    /// - `min_limit` does not exceed `max_limit`
    /// - `increase_by` is non-zero
    /// - `decrease_factor` is finite and in `[0.0, 1.0)`
    pub fn validate(&self) -> Result<(), AimdConfigError> {
        if self.min_limit > self.max_limit {
            return Err(AimdConfigError::MinExceedsMax {
                min_limit: self.min_limit,
                max_limit: self.max_limit,
            });
        }
        if self.increase_by == 0 {
            return Err(AimdConfigError::ZeroIncrease);
        }
        if !self.decrease_factor.is_finite() || !(0.0..1.0).contains(&self.decrease_factor) {
            return Err(AimdConfigError::InvalidDecreaseFactor(self.decrease_factor));
        }
        Ok(())
    }
}

/// Errors that can occur when validating an [`AimdConfig`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum AimdConfigError {
    /// `min_limit` is greater than `max_limit`.
    #[error("min_limit ({min_limit}) must not exceed max_limit ({max_limit})")]
    MinExceedsMax {
        /// Configured minimum limit.
        min_limit: usize,
        /// Configured maximum limit.
        max_limit: usize,
    },
    /// `increase_by` is zero, which would make the additive increase a no-op.
    #[error("increase_by must be greater than zero")]
    ZeroIncrease,
    /// `decrease_factor` is not finite, or not in the range `[0.0, 1.0)`.
    #[error("decrease_factor ({0}) must be finite and in the range [0.0, 1.0)")]
    InvalidDecreaseFactor(f64),
}

/// Thread-safe AIMD controller.
///
/// This controller manages a limit value that increases additively on success
/// and decreases multiplicatively on failure. It's commonly used for:
/// - Concurrency control
/// - Rate limiting
/// - Retry budgets
///
/// The controller is thread-safe and can be shared across tasks. Concurrent
/// observations are applied via compare-and-swap on the underlying atomic:
/// each call to `record_success`, `record_failure`, or `record_successes`
/// contributes exactly once, and a success racing a failure resolves to one
/// of the two possible serial orderings (no lost updates).
pub struct AimdController {
    /// Current limit value.
    limit: AtomicUsize,
    /// Configuration.
    config: AimdConfig,
}

impl AimdController {
    /// Create a new AIMD controller with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid (see
    /// [`AimdConfig::validate`]).
    pub fn new(config: AimdConfig) -> Result<Self, AimdConfigError> {
        config.validate()?;
        let initial = config
            .initial_limit
            .clamp(config.min_limit, config.max_limit);
        Ok(Self {
            limit: AtomicUsize::new(initial),
            config,
        })
    }

    /// Get the current limit.
    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Relaxed)
    }

    /// Get the minimum limit.
    pub fn min_limit(&self) -> usize {
        self.config.min_limit
    }

    /// Get the maximum limit.
    pub fn max_limit(&self) -> usize {
        self.config.max_limit
    }

    /// Record a success - increases the limit additively.
    pub fn record_success(&self) {
        let increase_by = self.config.increase_by;
        let max_limit = self.config.max_limit;
        self.limit
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_add(increase_by).min(max_limit))
            })
            .expect("update closure always returns Some");
    }

    /// Record a failure - decreases the limit multiplicatively.
    pub fn record_failure(&self) {
        let decrease_factor = self.config.decrease_factor;
        let min_limit = self.config.min_limit;
        self.limit
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                let decreased = (current as f64 * decrease_factor) as usize;
                Some(decreased.max(min_limit))
            })
            .expect("update closure always returns Some");
    }

    /// Record multiple successes at once.
    pub fn record_successes(&self, count: usize) {
        let increase = self.config.increase_by.saturating_mul(count);
        let max_limit = self.config.max_limit;
        self.limit
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_add(increase).min(max_limit))
            })
            .expect("update closure always returns Some");
    }

    /// Reset the limit to its initial value.
    pub fn reset(&self) {
        let initial = self
            .config
            .initial_limit
            .clamp(self.config.min_limit, self.config.max_limit);
        self.limit.store(initial, Ordering::Relaxed);
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &AimdConfig {
        &self.config
    }
}

impl Clone for AimdController {
    fn clone(&self) -> Self {
        Self {
            limit: AtomicUsize::new(self.limit.load(Ordering::Relaxed)),
            config: self.config.clone(),
        }
    }
}

impl std::fmt::Debug for AimdController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AimdController")
            .field("limit", &self.limit())
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AimdConfig::default();
        assert_eq!(config.initial_limit, 10);
        assert_eq!(config.min_limit, 1);
        assert_eq!(config.max_limit, 100);
        assert_eq!(config.increase_by, 1);
        assert!((config.decrease_factor - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_config_builder() {
        let config = AimdConfig::new()
            .with_initial_limit(50)
            .with_min_limit(10)
            .with_max_limit(200)
            .with_increase_by(5)
            .with_decrease_factor(0.75);

        assert_eq!(config.initial_limit, 50);
        assert_eq!(config.min_limit, 10);
        assert_eq!(config.max_limit, 200);
        assert_eq!(config.increase_by, 5);
        assert!((config.decrease_factor - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_controller_initial_limit() {
        let config = AimdConfig::default().with_initial_limit(25);
        let controller = AimdController::new(config).unwrap();
        assert_eq!(controller.limit(), 25);
    }

    #[test]
    fn test_controller_initial_clamped_to_max() {
        let config = AimdConfig::default()
            .with_initial_limit(200)
            .with_max_limit(50);
        let controller = AimdController::new(config).unwrap();
        assert_eq!(controller.limit(), 50);
    }

    #[test]
    fn test_controller_initial_clamped_to_min() {
        let config = AimdConfig::default()
            .with_initial_limit(0)
            .with_min_limit(5);
        let controller = AimdController::new(config).unwrap();
        assert_eq!(controller.limit(), 5);
    }

    #[test]
    fn test_additive_increase() {
        let config = AimdConfig::default()
            .with_initial_limit(10)
            .with_increase_by(2);
        let controller = AimdController::new(config).unwrap();

        controller.record_success();
        assert_eq!(controller.limit(), 12);

        controller.record_success();
        assert_eq!(controller.limit(), 14);
    }

    #[test]
    fn test_increase_respects_max() {
        let config = AimdConfig::default()
            .with_initial_limit(98)
            .with_max_limit(100)
            .with_increase_by(5);
        let controller = AimdController::new(config).unwrap();

        controller.record_success();
        assert_eq!(controller.limit(), 100); // Clamped to max
    }

    #[test]
    fn test_multiplicative_decrease() {
        let config = AimdConfig::default()
            .with_initial_limit(100)
            .with_decrease_factor(0.5);
        let controller = AimdController::new(config).unwrap();

        controller.record_failure();
        assert_eq!(controller.limit(), 50);

        controller.record_failure();
        assert_eq!(controller.limit(), 25);
    }

    #[test]
    fn test_decrease_respects_min() {
        let config = AimdConfig::default()
            .with_initial_limit(10)
            .with_min_limit(5)
            .with_decrease_factor(0.1);
        let controller = AimdController::new(config).unwrap();

        controller.record_failure();
        assert_eq!(controller.limit(), 5); // Clamped to min
    }

    #[test]
    fn test_on_successes_batch() {
        let config = AimdConfig::default()
            .with_initial_limit(10)
            .with_increase_by(1);
        let controller = AimdController::new(config).unwrap();

        controller.record_successes(5);
        assert_eq!(controller.limit(), 15);
    }

    #[test]
    fn test_reset() {
        let config = AimdConfig::default().with_initial_limit(50);
        let controller = AimdController::new(config).unwrap();

        controller.record_success();
        controller.record_success();
        assert_eq!(controller.limit(), 52);

        controller.reset();
        assert_eq!(controller.limit(), 50);
    }

    #[test]
    fn test_sawtooth_pattern() {
        // Simulate the classic AIMD sawtooth pattern
        let config = AimdConfig::default()
            .with_initial_limit(10)
            .with_min_limit(1)
            .with_max_limit(100)
            .with_increase_by(1)
            .with_decrease_factor(0.5);
        let controller = AimdController::new(config).unwrap();

        // Increase 10 times
        for _ in 0..10 {
            controller.record_success();
        }
        assert_eq!(controller.limit(), 20);

        // Failure halves it
        controller.record_failure();
        assert_eq!(controller.limit(), 10);

        // Increase again
        for _ in 0..20 {
            controller.record_success();
        }
        assert_eq!(controller.limit(), 30);

        // Another failure
        controller.record_failure();
        assert_eq!(controller.limit(), 15);
    }

    #[test]
    fn test_clone() {
        let config = AimdConfig::default().with_initial_limit(42);
        let controller = AimdController::new(config).unwrap();
        controller.record_success();

        let cloned = controller.clone();
        assert_eq!(cloned.limit(), 43);

        // Changes to original don't affect clone
        controller.record_failure();
        assert_eq!(controller.limit(), 21);
        assert_eq!(cloned.limit(), 43);
    }

    #[test]
    fn test_debug() {
        let config = AimdConfig::default().with_initial_limit(10);
        let controller = AimdController::new(config).unwrap();
        let debug_str = format!("{:?}", controller);
        assert!(debug_str.contains("AimdController"));
        assert!(debug_str.contains("10"));
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let config = AimdConfig::default()
            .with_initial_limit(1000)
            .with_max_limit(10000);
        let controller = Arc::new(AimdController::new(config).unwrap());

        let mut handles = vec![];

        // Spawn threads that increment
        for _ in 0..10 {
            let c = Arc::clone(&controller);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    c.record_success();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have increased (exact value depends on race conditions)
        assert!(controller.limit() > 1000);
    }

    #[test]
    fn test_validate_min_exceeds_max() {
        let config = AimdConfig::default().with_min_limit(50).with_max_limit(10);
        let err = AimdController::new(config).unwrap_err();
        assert_eq!(
            err,
            AimdConfigError::MinExceedsMax {
                min_limit: 50,
                max_limit: 10,
            }
        );
    }

    #[test]
    fn test_validate_zero_increase() {
        let config = AimdConfig::default().with_increase_by(0);
        let err = AimdController::new(config).unwrap_err();
        assert_eq!(err, AimdConfigError::ZeroIncrease);
    }

    #[test]
    fn test_validate_invalid_decrease_factor() {
        for factor in [-1.0, 1.0, f64::INFINITY, f64::NAN] {
            let config = AimdConfig::default().with_decrease_factor(factor);
            let err = AimdController::new(config).unwrap_err();
            assert!(matches!(err, AimdConfigError::InvalidDecreaseFactor(_)));
        }
    }

    #[test]
    fn test_concurrent_record_success_no_lost_updates() {
        use std::sync::Arc;

        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 2000;

        let config = AimdConfig::default()
            .with_initial_limit(0)
            .with_min_limit(0)
            .with_max_limit(usize::MAX / 2)
            .with_increase_by(1);
        let controller = Arc::new(AimdController::new(config).unwrap());

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                let c = Arc::clone(&controller);
                s.spawn(move || {
                    for _ in 0..CALLS_PER_THREAD {
                        c.record_success();
                    }
                });
            }
        });

        // No clamping occurs (max_limit is huge), so every observation must
        // have been applied for the total to match exactly.
        assert_eq!(controller.limit(), THREADS * CALLS_PER_THREAD);
    }

    #[test]
    fn test_concurrent_record_failure_matches_serial_application() {
        use std::sync::Arc;

        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 200;

        let make_config = || {
            AimdConfig::default()
                .with_initial_limit(1_000_000)
                .with_min_limit(1)
                .with_max_limit(2_000_000)
                .with_decrease_factor(0.999)
        };

        let concurrent = Arc::new(AimdController::new(make_config()).unwrap());
        std::thread::scope(|s| {
            for _ in 0..THREADS {
                let c = Arc::clone(&concurrent);
                s.spawn(move || {
                    for _ in 0..CALLS_PER_THREAD {
                        c.record_failure();
                    }
                });
            }
        });

        let serial = AimdController::new(make_config()).unwrap();
        for _ in 0..(THREADS * CALLS_PER_THREAD) {
            serial.record_failure();
        }

        // Every `record_failure` call applies the same deterministic
        // multiply-and-clamp function, so applying it N times via any
        // interleaving of concurrent CAS updates yields the same result as
        // applying it N times serially.
        assert_eq!(concurrent.limit(), serial.limit());
    }

    #[test]
    fn test_concurrent_mixed_success_and_failure_stays_within_bounds() {
        use std::sync::Arc;

        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 500;

        let config = AimdConfig::default()
            .with_initial_limit(50)
            .with_min_limit(1)
            .with_max_limit(100)
            .with_increase_by(1)
            .with_decrease_factor(0.5);
        let controller = Arc::new(AimdController::new(config).unwrap());

        std::thread::scope(|s| {
            for i in 0..THREADS {
                let c = Arc::clone(&controller);
                s.spawn(move || {
                    for j in 0..CALLS_PER_THREAD {
                        if (i + j) % 2 == 0 {
                            c.record_success();
                        } else {
                            c.record_failure();
                        }
                    }
                });
            }
        });

        let final_limit = controller.limit();
        assert!(final_limit >= controller.min_limit());
        assert!(final_limit <= controller.max_limit());
    }
}
