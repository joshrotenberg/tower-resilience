use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Internal state for the rate limiter.
#[derive(Debug)]
pub(crate) struct RateLimiterState {
    /// Maximum number of permits allowed per refresh period.
    limit_for_period: usize,
    /// Duration of the refresh period.
    refresh_period: Duration,
    /// How long to wait for a permit before giving up.
    timeout_duration: Duration,
    /// Currently available permits.
    available_permits: usize,
    /// When the current period started.
    period_start: Instant,
}

impl RateLimiterState {
    pub(crate) fn new(
        limit_for_period: usize,
        refresh_period: Duration,
        timeout_duration: Duration,
    ) -> Self {
        Self {
            limit_for_period,
            refresh_period,
            timeout_duration,
            available_permits: limit_for_period,
            period_start: Instant::now(),
        }
    }

    /// Attempts to acquire a permit, refreshing if needed.
    /// Returns Ok(wait_duration) if acquired, Err if timeout would be exceeded.
    pub(crate) fn try_acquire(&mut self) -> Result<Duration, Duration> {
        let now = Instant::now();

        // Check if we need to refresh the period
        if now.duration_since(self.period_start) >= self.refresh_period {
            self.refresh(now);
        }

        // If permits available, grant immediately
        if self.available_permits > 0 {
            self.available_permits -= 1;
            return Ok(Duration::ZERO);
        }

        // No permits available - calculate wait time
        let time_until_refresh = self
            .refresh_period
            .saturating_sub(now.duration_since(self.period_start));

        // Check if wait time exceeds timeout
        if time_until_refresh > self.timeout_duration {
            Err(self.timeout_duration)
        } else {
            Ok(time_until_refresh)
        }
    }

    /// Refreshes the permit pool for a new period.
    fn refresh(&mut self, now: Instant) {
        self.available_permits = self.limit_for_period;
        self.period_start = now;
    }

    /// Returns the current number of available permits.
    #[allow(dead_code)]
    pub(crate) fn available_permits(&self) -> usize {
        self.available_permits
    }
}

/// Shared rate limiter that can be cloned across services.
#[derive(Debug, Clone)]
pub(crate) struct SharedRateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
}

impl SharedRateLimiter {
    pub(crate) fn new(
        limit_for_period: usize,
        refresh_period: Duration,
        timeout_duration: Duration,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimiterState::new(
                limit_for_period,
                refresh_period,
                timeout_duration,
            ))),
        }
    }

    /// Attempts to acquire a permit.
    /// Returns Ok(duration_waited) if successful, Err if rate limited.
    pub(crate) async fn acquire(&self) -> Result<Duration, ()> {
        let result = {
            let mut state = self.state.lock().unwrap();
            state.try_acquire()
        };

        match result {
            Ok(Duration::ZERO) => {
                // Got permit immediately
                Ok(Duration::ZERO)
            }
            Ok(wait_duration) => {
                // Need to wait for next refresh
                sleep(wait_duration).await;

                // Try again after waiting
                let mut state = self.state.lock().unwrap();
                match state.try_acquire() {
                    Ok(total_wait) => Ok(wait_duration + total_wait),
                    Err(_) => Err(()), // Timeout exceeded
                }
            }
            Err(_) => {
                // Timeout would be exceeded
                Err(())
            }
        }
    }

    /// Returns the current number of available permits.
    #[allow(dead_code)]
    pub(crate) fn available_permits(&self) -> usize {
        self.state.lock().unwrap().available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_permits() {
        let state = RateLimiterState::new(10, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_acquire_permit() {
        let mut state =
            RateLimiterState::new(10, Duration::from_secs(1), Duration::from_millis(100));

        // Should succeed immediately
        let result = state.try_acquire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::ZERO);
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_exhaust_permits() {
        let mut state =
            RateLimiterState::new(2, Duration::from_millis(100), Duration::from_secs(1));

        // Acquire both permits
        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed (wait < timeout)
        let result = state.try_acquire();
        assert!(result.is_ok()); // Wait duration (100ms) should be less than timeout (1s)
    }

    #[test]
    fn test_refresh_restores_permits() {
        let mut state = RateLimiterState::new(5, Duration::from_millis(10), Duration::from_secs(1));

        // Exhaust permits
        for _ in 0..5 {
            state.try_acquire().unwrap();
        }
        assert_eq!(state.available_permits(), 0);

        // Wait for refresh period
        std::thread::sleep(Duration::from_millis(15));

        // Should be able to acquire again after refresh
        let result = state.try_acquire();
        assert!(result.is_ok());
        // Available permits should be refreshed minus the one we just acquired
        assert!(state.available_permits() > 0);
    }

    #[tokio::test]
    async fn test_shared_limiter() {
        let limiter = SharedRateLimiter::new(2, Duration::from_secs(1), Duration::from_millis(100));

        // Should acquire successfully
        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 1);

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }
}
