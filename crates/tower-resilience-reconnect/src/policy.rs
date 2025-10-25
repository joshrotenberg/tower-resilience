//! Reconnection policies defining backoff strategies.

use std::sync::Arc;
use std::time::Duration;
use tower_resilience_retry::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, IntervalFunction,
};

/// Reconnection policy defining how to backoff between reconnection attempts.
pub enum ReconnectPolicy {
    /// No automatic reconnection
    None,

    /// Fixed delay between reconnection attempts
    Fixed(FixedInterval),

    /// Exponential backoff between attempts
    Exponential(ExponentialBackoff),

    /// Exponential backoff with randomization to prevent thundering herd
    ExponentialRandom(ExponentialRandomBackoff),

    /// Custom backoff function
    Custom(Arc<dyn IntervalFunction>),
}

impl Clone for ReconnectPolicy {
    fn clone(&self) -> Self {
        match self {
            Self::None => Self::None,
            Self::Fixed(f) => Self::Fixed(f.clone()),
            Self::Exponential(e) => Self::Exponential(e.clone()),
            Self::ExponentialRandom(e) => Self::ExponentialRandom(e.clone()),
            Self::Custom(c) => Self::Custom(c.clone()),
        }
    }
}

impl ReconnectPolicy {
    /// Create a policy with no reconnection
    pub fn none() -> Self {
        ReconnectPolicy::None
    }

    /// Create a fixed delay policy
    pub fn fixed(delay: Duration) -> Self {
        ReconnectPolicy::Fixed(FixedInterval::new(delay))
    }

    /// Create an exponential backoff policy
    ///
    /// # Arguments
    /// * `initial_delay` - Starting delay (e.g., 100ms)
    /// * `max_delay` - Maximum delay cap (e.g., 5 seconds)
    pub fn exponential(initial_delay: Duration, max_delay: Duration) -> Self {
        ReconnectPolicy::Exponential(
            ExponentialBackoff::new(initial_delay)
                .multiplier(2.0)
                .max_interval(max_delay),
        )
    }

    /// Create an exponential backoff policy with randomization
    ///
    /// # Arguments
    /// * `initial_delay` - Starting delay
    /// * `max_delay` - Maximum delay cap
    /// * `randomization_factor` - Randomization factor (0.0 to 1.0)
    pub fn exponential_random(
        initial_delay: Duration,
        max_delay: Duration,
        randomization_factor: f64,
    ) -> Self {
        ReconnectPolicy::ExponentialRandom(
            ExponentialRandomBackoff::new(initial_delay, randomization_factor)
                .multiplier(2.0)
                .max_interval(max_delay),
        )
    }

    /// Get the delay for a given attempt number
    pub fn delay_for_attempt(&self, attempt: usize) -> Option<Duration> {
        match self {
            ReconnectPolicy::None => None,
            ReconnectPolicy::Fixed(interval) => Some(interval.next_interval(attempt)),
            ReconnectPolicy::Exponential(backoff) => Some(backoff.next_interval(attempt)),
            ReconnectPolicy::ExponentialRandom(backoff) => Some(backoff.next_interval(attempt)),
            ReconnectPolicy::Custom(func) => Some(func.next_interval(attempt)),
        }
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        // Default: Exponential backoff from 100ms to 5 seconds
        Self::exponential(Duration::from_millis(100), Duration::from_secs(5))
    }
}

impl std::fmt::Debug for ReconnectPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "ReconnectPolicy::None"),
            Self::Fixed(_) => write!(f, "ReconnectPolicy::Fixed"),
            Self::Exponential(_) => write!(f, "ReconnectPolicy::Exponential"),
            Self::ExponentialRandom(_) => write!(f, "ReconnectPolicy::ExponentialRandom"),
            Self::Custom(_) => write!(f, "ReconnectPolicy::Custom"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_policy() {
        let policy = ReconnectPolicy::none();
        assert!(policy.delay_for_attempt(0).is_none());
        assert!(policy.delay_for_attempt(1).is_none());
    }

    #[test]
    fn test_fixed_policy() {
        let policy = ReconnectPolicy::fixed(Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(0), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay_for_attempt(10), Some(Duration::from_secs(1)));
    }

    #[test]
    fn test_exponential_policy() {
        let policy =
            ReconnectPolicy::exponential(Duration::from_millis(100), Duration::from_secs(1));

        assert_eq!(
            policy.delay_for_attempt(0),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            policy.delay_for_attempt(1),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            policy.delay_for_attempt(2),
            Some(Duration::from_millis(400))
        );
        assert_eq!(
            policy.delay_for_attempt(3),
            Some(Duration::from_millis(800))
        );
        // Should cap at max_delay (1 second)
        assert_eq!(policy.delay_for_attempt(4), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay_for_attempt(10), Some(Duration::from_secs(1)));
    }

    #[test]
    fn test_default_policy() {
        let policy = ReconnectPolicy::default();
        // Should be exponential with reasonable defaults
        let delay = policy.delay_for_attempt(0);
        assert!(delay.is_some());
        assert_eq!(delay.unwrap(), Duration::from_millis(100));
    }
}
