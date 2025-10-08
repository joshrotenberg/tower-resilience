use std::time::Duration;

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
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Sets the maximum interval to cap exponential growth.
    pub fn max_interval(mut self, max_interval: Duration) -> Self {
        self.max_interval = Some(max_interval);
        self
    }
}

impl IntervalFunction for ExponentialBackoff {
    fn next_interval(&self, attempt: usize) -> Duration {
        let multiplier = self.multiplier.powi(attempt as i32);
        let interval = self.initial_interval.mul_f64(multiplier);

        if let Some(max) = self.max_interval {
            interval.min(max)
        } else {
            interval
        }
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
    pub fn new(initial_interval: Duration, randomization_factor: f64) -> Self {
        Self {
            initial_interval,
            multiplier: 2.0,
            randomization_factor: randomization_factor.clamp(0.0, 1.0),
            max_interval: None,
        }
    }

    /// Sets the multiplier for exponential growth.
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Sets the maximum interval to cap exponential growth.
    pub fn max_interval(mut self, max_interval: Duration) -> Self {
        self.max_interval = Some(max_interval);
        self
    }

    fn randomize(&self, duration: Duration) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let delta = duration.as_secs_f64() * self.randomization_factor;
        let min = duration.as_secs_f64() - delta;
        let max = duration.as_secs_f64() + delta;
        let randomized = rng.gen_range(min..=max);
        Duration::from_secs_f64(randomized.max(0.0))
    }
}

impl IntervalFunction for ExponentialRandomBackoff {
    fn next_interval(&self, attempt: usize) -> Duration {
        let multiplier = self.multiplier.powi(attempt as i32);
        let interval = self.initial_interval.mul_f64(multiplier);

        let capped = if let Some(max) = self.max_interval {
            interval.min(max)
        } else {
            interval
        };

        self.randomize(capped)
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
        let backoff = ExponentialBackoff::new(Duration::from_millis(100)).multiplier(3.0);
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
        let backoff = ExponentialRandomBackoff::new(Duration::from_millis(100), 0.5);

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
    fn fn_interval_uses_custom_function() {
        let backoff = FnInterval::new(|attempt| Duration::from_secs((attempt + 1) as u64));
        assert_eq!(backoff.next_interval(0), Duration::from_secs(1));
        assert_eq!(backoff.next_interval(1), Duration::from_secs(2));
        assert_eq!(backoff.next_interval(2), Duration::from_secs(3));
    }
}
