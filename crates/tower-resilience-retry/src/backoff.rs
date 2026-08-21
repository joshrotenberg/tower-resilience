use std::time::Duration;

/// Errors returned when configuring a built-in retry backoff strategy.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum BackoffConfigError {
    /// The exponential multiplier is zero, negative, or non-finite.
    #[error("multiplier ({0}) must be finite and greater than zero")]
    InvalidMultiplier(f64),

    /// The randomization factor is non-finite or outside the inclusive range `0.0..=1.0`.
    #[error("randomization_factor ({0}) must be finite and in the range [0.0, 1.0]")]
    InvalidRandomizationFactor(f64),
}

/// Abstraction for computing retry intervals.
///
/// This trait allows for flexible backoff strategies including fixed delays,
/// exponential backoff, randomized backoff, and custom implementations.
pub trait IntervalFunction: Send + Sync {
    /// Computes the delay before the next retry attempt.
    ///
    /// # Arguments
    /// * `attempt` - The retry attempt number (0-indexed, so first retry is 0)
    fn next_interval(&self, attempt: usize) -> Duration;
}

/// Fixed interval backoff - returns the same duration for every retry.
#[derive(Debug, Clone)]
pub struct FixedInterval {
    duration: Duration,
}

impl FixedInterval {
    /// Creates a new fixed interval backoff.
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }
}

impl IntervalFunction for FixedInterval {
    fn next_interval(&self, _attempt: usize) -> Duration {
        self.duration
    }
}

/// Exponential backoff with configurable multiplier.
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    initial_interval: Duration,
    multiplier: f64,
    max_interval: Option<Duration>,
}

impl ExponentialBackoff {
    /// Creates a new exponential backoff with default multiplier of 2.0.
    pub fn new(initial_interval: Duration) -> Self {
        Self {
            initial_interval,
            multiplier: 2.0,
            max_interval: None,
        }
    }

    /// Sets the multiplier for exponential growth.
    ///
    /// Values between zero and one produce decreasing intervals. A multiplier
    /// of zero is rejected because it collapses every interval after the first
    /// to zero and cannot represent exponential scaling.
    ///
    /// # Errors
    ///
    /// Returns [`BackoffConfigError::InvalidMultiplier`] if `multiplier` is
    /// zero, negative, or non-finite.
    pub fn multiplier(mut self, multiplier: f64) -> Result<Self, BackoffConfigError> {
        validate_multiplier(multiplier)?;
        self.multiplier = multiplier;
        Ok(self)
    }

    /// Sets the maximum interval to cap exponential growth.
    pub fn max_interval(mut self, max_interval: Duration) -> Self {
        self.max_interval = Some(max_interval);
        self
    }
}

impl IntervalFunction for ExponentialBackoff {
    fn next_interval(&self, attempt: usize) -> Duration {
        scaled_interval(
            self.initial_interval,
            self.multiplier,
            attempt,
            self.max_interval,
        )
    }
}

/// Exponential backoff with randomization to prevent thundering herd.
#[derive(Debug, Clone)]
pub struct ExponentialRandomBackoff {
    initial_interval: Duration,
    multiplier: f64,
    randomization_factor: f64,
    max_interval: Option<Duration>,
}

impl ExponentialRandomBackoff {
    /// Creates a new exponential random backoff.
    ///
    /// # Arguments
    /// * `initial_interval` - The base interval
    /// * `randomization_factor` - Factor for randomization (0.0 to 1.0)
    ///   A factor of 0.5 means the interval will be randomized between 50% and 150% of the calculated value.
    ///
    /// # Errors
    ///
    /// Returns [`BackoffConfigError::InvalidRandomizationFactor`] if
    /// `randomization_factor` is non-finite or outside `0.0..=1.0`.
    pub fn new(
        initial_interval: Duration,
        randomization_factor: f64,
    ) -> Result<Self, BackoffConfigError> {
        validate_randomization_factor(randomization_factor)?;
        Ok(Self {
            initial_interval,
            multiplier: 2.0,
            randomization_factor,
            max_interval: None,
        })
    }

    /// Sets the multiplier for exponential growth.
    ///
    /// # Errors
    ///
    /// Returns [`BackoffConfigError::InvalidMultiplier`] if `multiplier` is
    /// zero, negative, or non-finite.
    pub fn multiplier(mut self, multiplier: f64) -> Result<Self, BackoffConfigError> {
        validate_multiplier(multiplier)?;
        self.multiplier = multiplier;
        Ok(self)
    }

    /// Sets the maximum interval to cap exponential growth.
    pub fn max_interval(mut self, max_interval: Duration) -> Self {
        self.max_interval = Some(max_interval);
        self
    }

    fn randomize(&self, duration: Duration) -> Duration {
        use rand::Rng;
        let mut rng = rand::rng();
        let delta = duration.as_secs_f64() * self.randomization_factor;
        let min = duration.as_secs_f64() - delta;
        let max = duration.as_secs_f64() + delta;
        let randomized = rng.random_range(min..=max);
        duration_from_secs_f64_saturating(randomized.max(0.0))
    }
}

impl IntervalFunction for ExponentialRandomBackoff {
    fn next_interval(&self, attempt: usize) -> Duration {
        let capped = scaled_interval(
            self.initial_interval,
            self.multiplier,
            attempt,
            self.max_interval,
        );
        self.randomize(capped)
    }
}

fn validate_multiplier(multiplier: f64) -> Result<(), BackoffConfigError> {
    if multiplier.is_finite() && multiplier > 0.0 {
        Ok(())
    } else {
        Err(BackoffConfigError::InvalidMultiplier(multiplier))
    }
}

fn validate_randomization_factor(randomization_factor: f64) -> Result<(), BackoffConfigError> {
    if randomization_factor.is_finite() && (0.0..=1.0).contains(&randomization_factor) {
        Ok(())
    } else {
        Err(BackoffConfigError::InvalidRandomizationFactor(
            randomization_factor,
        ))
    }
}

fn scaled_interval(
    initial_interval: Duration,
    multiplier: f64,
    attempt: usize,
    max_interval: Option<Duration>,
) -> Duration {
    if initial_interval.is_zero() {
        return Duration::ZERO;
    }

    let factor = multiplier.powf(attempt as f64);
    let seconds = initial_interval.as_secs_f64() * factor;

    if let Some(max_interval) = max_interval {
        if !seconds.is_finite() || seconds >= max_interval.as_secs_f64() {
            return max_interval;
        }
    }

    duration_from_secs_f64_saturating(seconds)
}

fn duration_from_secs_f64_saturating(seconds: f64) -> Duration {
    if !seconds.is_finite() {
        Duration::MAX
    } else if seconds <= 0.0 {
        Duration::ZERO
    } else {
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
    }
}

/// Function-based interval implementation.
pub struct FnInterval<F> {
    f: F,
}

impl<F> FnInterval<F>
where
    F: Fn(usize) -> Duration + Send + Sync,
{
    /// Creates a new function-based interval.
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F> IntervalFunction for FnInterval<F>
where
    F: Fn(usize) -> Duration + Send + Sync,
{
    fn next_interval(&self, attempt: usize) -> Duration {
        (self.f)(attempt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_interval_returns_same_duration() {
        let backoff = FixedInterval::new(Duration::from_secs(1));
        assert_eq!(backoff.next_interval(0), Duration::from_secs(1));
        assert_eq!(backoff.next_interval(1), Duration::from_secs(1));
        assert_eq!(backoff.next_interval(10), Duration::from_secs(1));
    }

    #[test]
    fn exponential_backoff_grows() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100));
        assert_eq!(backoff.next_interval(0), Duration::from_millis(100));
        assert_eq!(backoff.next_interval(1), Duration::from_millis(200));
        assert_eq!(backoff.next_interval(2), Duration::from_millis(400));
        assert_eq!(backoff.next_interval(3), Duration::from_millis(800));
    }

    #[test]
    fn exponential_backoff_custom_multiplier() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100))
            .multiplier(3.0)
            .unwrap();
        assert_eq!(backoff.next_interval(0), Duration::from_millis(100));
        assert_eq!(backoff.next_interval(1), Duration::from_millis(300));
        assert_eq!(backoff.next_interval(2), Duration::from_millis(900));
    }

    #[test]
    fn exponential_backoff_respects_max() {
        let backoff = ExponentialBackoff::new(Duration::from_millis(100))
            .max_interval(Duration::from_millis(500));
        assert_eq!(backoff.next_interval(0), Duration::from_millis(100));
        assert_eq!(backoff.next_interval(1), Duration::from_millis(200));
        assert_eq!(backoff.next_interval(2), Duration::from_millis(400));
        assert_eq!(backoff.next_interval(3), Duration::from_millis(500)); // capped
        assert_eq!(backoff.next_interval(4), Duration::from_millis(500)); // capped
    }

    #[test]
    fn exponential_random_backoff_has_variance() {
        let backoff = ExponentialRandomBackoff::new(Duration::from_millis(100), 0.5).unwrap();

        // Run multiple times to check randomization
        let mut intervals = Vec::new();
        for _ in 0..10 {
            intervals.push(backoff.next_interval(1));
        }

        // Should have some variance (not all the same)
        let all_same = intervals.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "Randomized intervals should vary");

        // All should be within expected range (100ms to 300ms for attempt 1)
        // Base: 200ms (100 * 2^1), with 0.5 factor: 100ms to 300ms
        for interval in intervals {
            assert!(
                interval >= Duration::from_millis(100) && interval <= Duration::from_millis(300),
                "Interval {:?} outside expected range",
                interval
            );
        }
    }

    #[test]
    fn exponential_backoff_rejects_invalid_multipliers() {
        for multiplier in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                ExponentialBackoff::new(Duration::from_millis(100)).multiplier(multiplier),
                Err(BackoffConfigError::InvalidMultiplier(value))
                    if value.to_bits() == multiplier.to_bits()
            ));

            assert!(matches!(
                ExponentialRandomBackoff::new(Duration::from_millis(100), 0.5)
                    .unwrap()
                    .multiplier(multiplier),
                Err(BackoffConfigError::InvalidMultiplier(value))
                    if value.to_bits() == multiplier.to_bits()
            ));
        }
    }

    #[test]
    fn exponential_random_backoff_rejects_invalid_randomization_factors() {
        for factor in [-0.1, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                ExponentialRandomBackoff::new(Duration::from_millis(100), factor),
                Err(BackoffConfigError::InvalidRandomizationFactor(value))
                    if value.to_bits() == factor.to_bits()
            ));
        }
    }

    #[test]
    fn exponential_random_backoff_accepts_randomization_boundaries() {
        assert!(ExponentialRandomBackoff::new(Duration::ZERO, 0.0).is_ok());
        assert!(ExponentialRandomBackoff::new(Duration::ZERO, 1.0).is_ok());
    }

    #[test]
    fn exponential_backoff_preserves_valid_edge_configuration() {
        let zero = ExponentialBackoff::new(Duration::ZERO);
        assert_eq!(zero.next_interval(usize::MAX), Duration::ZERO);

        let decreasing = ExponentialBackoff::new(Duration::from_secs(4))
            .multiplier(0.5)
            .unwrap();
        assert_eq!(decreasing.next_interval(1), Duration::from_secs(2));

        let lower_cap =
            ExponentialBackoff::new(Duration::from_secs(2)).max_interval(Duration::from_secs(1));
        assert_eq!(lower_cap.next_interval(0), Duration::from_secs(1));
    }

    #[test]
    fn exponential_backoff_saturates_overflowing_intervals() {
        let backoff = ExponentialBackoff::new(Duration::from_secs(1))
            .multiplier(f64::MAX)
            .unwrap();
        assert_eq!(backoff.next_interval(2), Duration::MAX);

        let capped = backoff.max_interval(Duration::from_secs(60));
        assert_eq!(capped.next_interval(2), Duration::from_secs(60));

        let randomized = ExponentialRandomBackoff::new(Duration::from_secs(1), 0.0)
            .unwrap()
            .multiplier(f64::MAX)
            .unwrap();
        assert_eq!(randomized.next_interval(2), Duration::MAX);
    }

    #[test]
    fn fn_interval_uses_custom_function() {
        let backoff = FnInterval::new(|attempt| Duration::from_secs((attempt + 1) as u64));
        assert_eq!(backoff.next_interval(0), Duration::from_secs(1));
        assert_eq!(backoff.next_interval(1), Duration::from_secs(2));
        assert_eq!(backoff.next_interval(2), Duration::from_secs(3));
    }
}
