use crate::config::WindowType;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Result of attempting to acquire a permit.
/// Ok(wait_duration) means permit acquired (possibly after waiting).
/// Err(timeout) means acquisition failed due to timeout.
type AcquireResult = Result<Duration, Duration>;

/// Fixed window rate limiter state.
///
/// Resets all permits at fixed interval boundaries.
#[derive(Debug)]
struct FixedWindowState {
    limit_for_period: usize,
    refresh_period: Duration,
    timeout_duration: Duration,
    available_permits: usize,
    period_start: Instant,
}

impl FixedWindowState {
    fn new(limit_for_period: usize, refresh_period: Duration, timeout_duration: Duration) -> Self {
        Self {
            limit_for_period,
            refresh_period,
            timeout_duration,
            available_permits: limit_for_period,
            period_start: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> AcquireResult {
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

    /// Attempts to acquire a permit without timeout enforcement.
    ///
    /// Returns `Duration::ZERO` if a permit was consumed, or the wait duration
    /// until a permit will be available.
    fn try_acquire_no_timeout(&mut self) -> Duration {
        let now = Instant::now();

        if now.duration_since(self.period_start) >= self.refresh_period {
            self.refresh(now);
        }

        if self.available_permits > 0 {
            self.available_permits -= 1;
            return Duration::ZERO;
        }

        self.refresh_period
            .saturating_sub(now.duration_since(self.period_start))
    }

    fn refresh(&mut self, now: Instant) {
        self.available_permits = self.limit_for_period;
        self.period_start = now;
    }

    fn available_permits(&self) -> usize {
        self.available_permits
    }
}

/// Sliding log rate limiter state.
///
/// Stores timestamps of each request and counts those within the window.
/// Provides precise rate limiting but uses O(n) memory.
#[derive(Debug)]
struct SlidingLogState {
    limit_for_period: usize,
    window_duration: Duration,
    timeout_duration: Duration,
    /// Timestamps of requests within the current window.
    request_log: VecDeque<Instant>,
}

impl SlidingLogState {
    fn new(limit_for_period: usize, window_duration: Duration, timeout_duration: Duration) -> Self {
        Self {
            limit_for_period,
            window_duration,
            timeout_duration,
            request_log: VecDeque::with_capacity(limit_for_period),
        }
    }

    fn try_acquire(&mut self) -> AcquireResult {
        let now = Instant::now();

        // Remove expired entries from the front
        while let Some(&timestamp) = self.request_log.front() {
            if now.duration_since(timestamp) >= self.window_duration {
                self.request_log.pop_front();
            } else {
                break;
            }
        }

        // Check if we have capacity
        if self.request_log.len() < self.limit_for_period {
            self.request_log.push_back(now);
            return Ok(Duration::ZERO);
        }

        // No capacity - calculate when the oldest request will expire
        if let Some(&oldest) = self.request_log.front() {
            let time_until_slot = oldest
                .checked_add(self.window_duration)
                .map(|expiry| expiry.saturating_duration_since(now))
                .unwrap_or(Duration::ZERO);

            if time_until_slot > self.timeout_duration {
                Err(self.timeout_duration)
            } else {
                Ok(time_until_slot)
            }
        } else {
            // Should not happen if limit > 0
            Ok(Duration::ZERO)
        }
    }

    /// Attempts to acquire a permit without timeout enforcement.
    fn try_acquire_no_timeout(&mut self) -> Duration {
        let now = Instant::now();

        while let Some(&timestamp) = self.request_log.front() {
            if now.duration_since(timestamp) >= self.window_duration {
                self.request_log.pop_front();
            } else {
                break;
            }
        }

        if self.request_log.len() < self.limit_for_period {
            self.request_log.push_back(now);
            return Duration::ZERO;
        }

        if let Some(&oldest) = self.request_log.front() {
            oldest
                .checked_add(self.window_duration)
                .map(|expiry| expiry.saturating_duration_since(now))
                .unwrap_or(Duration::ZERO)
        } else {
            Duration::ZERO
        }
    }

    fn available_permits(&self) -> usize {
        self.limit_for_period.saturating_sub(self.request_log.len())
    }
}

/// Sliding window counter rate limiter state.
///
/// Uses weighted averaging between current and previous buckets.
/// Provides approximate sliding window with O(1) memory.
#[derive(Debug)]
struct SlidingCounterState {
    limit_for_period: usize,
    bucket_duration: Duration,
    timeout_duration: Duration,
    /// Count of requests in the previous bucket.
    previous_count: usize,
    /// Count of requests in the current bucket.
    current_count: usize,
    /// When the current bucket started.
    bucket_start: Instant,
}

impl SlidingCounterState {
    fn new(limit_for_period: usize, bucket_duration: Duration, timeout_duration: Duration) -> Self {
        Self {
            limit_for_period,
            bucket_duration,
            timeout_duration,
            previous_count: 0,
            current_count: 0,
            bucket_start: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> AcquireResult {
        let now = Instant::now();
        self.maybe_rotate_bucket(now);

        // Calculate weighted count
        let elapsed = now.duration_since(self.bucket_start);
        let elapsed_ratio = elapsed.as_secs_f64() / self.bucket_duration.as_secs_f64();
        let elapsed_ratio = elapsed_ratio.clamp(0.0, 1.0);

        // Weight: previous bucket contributes less as we progress through current bucket
        let previous_weight = 1.0 - elapsed_ratio;
        let weighted_count =
            (self.previous_count as f64 * previous_weight) + self.current_count as f64;

        if weighted_count < self.limit_for_period as f64 {
            self.current_count += 1;
            return Ok(Duration::ZERO);
        }

        // No capacity - estimate when a slot will be available
        // As time progresses, previous_weight decreases, freeing up capacity
        let time_until_slot = self.estimate_wait_time(elapsed_ratio);

        if time_until_slot > self.timeout_duration {
            Err(self.timeout_duration)
        } else {
            Ok(time_until_slot)
        }
    }

    /// Attempts to acquire a permit without timeout enforcement.
    fn try_acquire_no_timeout(&mut self) -> Duration {
        let now = Instant::now();
        self.maybe_rotate_bucket(now);

        let elapsed = now.duration_since(self.bucket_start);
        let elapsed_ratio = elapsed.as_secs_f64() / self.bucket_duration.as_secs_f64();
        let elapsed_ratio = elapsed_ratio.clamp(0.0, 1.0);

        let previous_weight = 1.0 - elapsed_ratio;
        let weighted_count =
            (self.previous_count as f64 * previous_weight) + self.current_count as f64;

        if weighted_count < self.limit_for_period as f64 {
            self.current_count += 1;
            return Duration::ZERO;
        }

        self.estimate_wait_time(elapsed_ratio)
    }

    fn maybe_rotate_bucket(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.bucket_start);

        if elapsed >= self.bucket_duration {
            // How many full buckets have passed?
            let buckets_passed =
                (elapsed.as_secs_f64() / self.bucket_duration.as_secs_f64()) as u32;

            if buckets_passed >= 2 {
                // More than one full bucket passed - previous is now empty
                self.previous_count = 0;
                self.current_count = 0;
            } else {
                // Exactly one bucket passed - rotate
                self.previous_count = self.current_count;
                self.current_count = 0;
            }

            self.bucket_start = now;
        }
    }

    fn estimate_wait_time(&self, current_ratio: f64) -> Duration {
        // We need weighted_count < limit
        // weighted = previous * (1 - ratio) + current
        // As ratio increases, previous contribution decreases
        // Solve for ratio where weighted = limit - 1 (to have room for one more)

        let limit = self.limit_for_period as f64;
        let current = self.current_count as f64;
        let previous = self.previous_count as f64;

        if previous == 0.0 {
            // No previous bucket contribution, need to wait for bucket rotation
            let remaining = self.bucket_duration.as_secs_f64() * (1.0 - current_ratio);
            return Duration::from_secs_f64(remaining);
        }

        // weighted = previous * (1 - ratio) + current = limit - epsilon
        // previous - previous * ratio + current = limit - epsilon
        // previous * ratio = previous + current - limit + epsilon
        // ratio = (previous + current - limit + epsilon) / previous
        let target_ratio = (previous + current - limit + 0.1) / previous;

        if target_ratio <= current_ratio {
            // Already past the point where we'd have capacity
            Duration::ZERO
        } else if target_ratio >= 1.0 {
            // Need to wait for bucket rotation
            let remaining = self.bucket_duration.as_secs_f64() * (1.0 - current_ratio);
            Duration::from_secs_f64(remaining)
        } else {
            let wait_ratio = target_ratio - current_ratio;
            Duration::from_secs_f64(wait_ratio * self.bucket_duration.as_secs_f64())
        }
    }

    fn available_permits(&self) -> usize {
        let now = Instant::now();
        let elapsed = now.duration_since(self.bucket_start);
        let elapsed_ratio =
            (elapsed.as_secs_f64() / self.bucket_duration.as_secs_f64()).clamp(0.0, 1.0);
        let previous_weight = 1.0 - elapsed_ratio;
        let weighted_count =
            (self.previous_count as f64 * previous_weight) + self.current_count as f64;

        self.limit_for_period
            .saturating_sub(weighted_count.ceil() as usize)
    }
}

/// Enum-based rate limiter state that dispatches to the appropriate implementation.
#[derive(Debug)]
enum RateLimiterStateInner {
    Fixed(FixedWindowState),
    SlidingLog(SlidingLogState),
    SlidingCounter(SlidingCounterState),
}

impl RateLimiterStateInner {
    fn new(
        window_type: WindowType,
        limit_for_period: usize,
        refresh_period: Duration,
        timeout_duration: Duration,
    ) -> Self {
        match window_type {
            WindowType::Fixed => Self::Fixed(FixedWindowState::new(
                limit_for_period,
                refresh_period,
                timeout_duration,
            )),
            WindowType::SlidingLog => Self::SlidingLog(SlidingLogState::new(
                limit_for_period,
                refresh_period,
                timeout_duration,
            )),
            WindowType::SlidingCounter => Self::SlidingCounter(SlidingCounterState::new(
                limit_for_period,
                refresh_period,
                timeout_duration,
            )),
        }
    }

    fn try_acquire(&mut self) -> AcquireResult {
        match self {
            Self::Fixed(state) => state.try_acquire(),
            Self::SlidingLog(state) => state.try_acquire(),
            Self::SlidingCounter(state) => state.try_acquire(),
        }
    }

    fn try_acquire_no_timeout(&mut self) -> Duration {
        match self {
            Self::Fixed(state) => state.try_acquire_no_timeout(),
            Self::SlidingLog(state) => state.try_acquire_no_timeout(),
            Self::SlidingCounter(state) => state.try_acquire_no_timeout(),
        }
    }

    fn available_permits(&self) -> usize {
        match self {
            Self::Fixed(state) => state.available_permits(),
            Self::SlidingLog(state) => state.available_permits(),
            Self::SlidingCounter(state) => state.available_permits(),
        }
    }
}

/// Shared rate limiter that can be cloned across services.
#[derive(Debug, Clone)]
pub(crate) struct SharedRateLimiter {
    state: Arc<Mutex<RateLimiterStateInner>>,
}

impl SharedRateLimiter {
    pub(crate) fn new(
        window_type: WindowType,
        limit_for_period: usize,
        refresh_period: Duration,
        timeout_duration: Duration,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimiterStateInner::new(
                window_type,
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
                // Need to wait
                sleep(wait_duration).await;

                // Try again after waiting
                let mut state = self.state.lock().unwrap();
                match state.try_acquire() {
                    Ok(additional_wait) => Ok(wait_duration + additional_wait),
                    Err(_) => Err(()), // Timeout exceeded
                }
            }
            Err(_) => {
                // Timeout would be exceeded
                Err(())
            }
        }
    }

    /// Attempts to acquire a permit immediately without waiting or timeout.
    ///
    /// Returns `Ok(())` if a permit was consumed, or `Err(wait_duration)` indicating
    /// how long to wait before retrying.
    pub(crate) fn try_acquire_now(&self) -> Result<(), Duration> {
        let mut state = self.state.lock().unwrap();
        let wait = state.try_acquire_no_timeout();
        if wait == Duration::ZERO {
            Ok(())
        } else {
            Err(wait)
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

    // ==================== Fixed Window Tests ====================

    #[test]
    fn test_fixed_initial_permits() {
        let state = FixedWindowState::new(10, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_fixed_acquire_permit() {
        let mut state =
            FixedWindowState::new(10, Duration::from_secs(1), Duration::from_millis(100));

        let result = state.try_acquire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::ZERO);
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_fixed_exhaust_permits() {
        let mut state =
            FixedWindowState::new(2, Duration::from_millis(100), Duration::from_secs(1));

        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        let result = state.try_acquire();
        assert!(result.is_ok());
    }

    #[test]
    fn test_fixed_refresh_restores_permits() {
        let mut state = FixedWindowState::new(5, Duration::from_millis(10), Duration::from_secs(1));

        for _ in 0..5 {
            state.try_acquire().unwrap();
        }
        assert_eq!(state.available_permits(), 0);

        std::thread::sleep(Duration::from_millis(15));

        let result = state.try_acquire();
        assert!(result.is_ok());
        assert!(state.available_permits() > 0);
    }

    // ==================== Sliding Log Tests ====================

    #[test]
    fn test_sliding_log_initial_permits() {
        let state = SlidingLogState::new(10, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_sliding_log_acquire_permit() {
        let mut state =
            SlidingLogState::new(10, Duration::from_secs(1), Duration::from_millis(100));

        let result = state.try_acquire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::ZERO);
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_sliding_log_exhaust_permits() {
        let mut state = SlidingLogState::new(2, Duration::from_millis(100), Duration::from_secs(1));

        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        let result = state.try_acquire();
        assert!(result.is_ok());
        assert!(result.unwrap() > Duration::ZERO);
    }

    #[test]
    fn test_sliding_log_expires_old_requests() {
        let mut state = SlidingLogState::new(2, Duration::from_millis(50), Duration::from_secs(1));

        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());
        assert_eq!(state.available_permits(), 0);

        // Wait for requests to expire
        std::thread::sleep(Duration::from_millis(60));

        // Should be able to acquire again
        let result = state.try_acquire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::ZERO);
    }

    #[test]
    fn test_sliding_log_no_boundary_burst() {
        let mut state =
            SlidingLogState::new(2, Duration::from_millis(100), Duration::from_millis(50));

        // Acquire 2 permits
        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());

        // Wait 60ms (past fixed window boundary but within sliding window)
        std::thread::sleep(Duration::from_millis(60));

        // With sliding log, these requests are still in the window
        // so we should NOT be able to acquire more (unlike fixed window)
        let result = state.try_acquire();
        // Should either need to wait or timeout
        assert!(result.is_ok()); // Returns wait duration
        assert!(result.unwrap() > Duration::ZERO || state.available_permits() < 2);
    }

    // ==================== Sliding Counter Tests ====================

    #[test]
    fn test_sliding_counter_initial_permits() {
        let state =
            SlidingCounterState::new(10, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_sliding_counter_acquire_permit() {
        let mut state =
            SlidingCounterState::new(10, Duration::from_secs(1), Duration::from_millis(100));

        let result = state.try_acquire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::ZERO);
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_sliding_counter_exhaust_permits() {
        let mut state =
            SlidingCounterState::new(2, Duration::from_millis(100), Duration::from_secs(1));

        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        let result = state.try_acquire();
        assert!(result.is_ok());
        assert!(result.unwrap() > Duration::ZERO);
    }

    #[test]
    fn test_sliding_counter_bucket_rotation() {
        let mut state =
            SlidingCounterState::new(2, Duration::from_millis(50), Duration::from_secs(1));

        assert!(state.try_acquire().is_ok());
        assert!(state.try_acquire().is_ok());

        // Wait for bucket to rotate
        std::thread::sleep(Duration::from_millis(55));

        // After rotation, previous_count = 2, current_count = 0
        // At start of new bucket, weighted = 2 * 1.0 + 0 = 2, so still at limit
        // But as time progresses, previous weight decreases

        // Wait a bit more for previous contribution to decrease
        std::thread::sleep(Duration::from_millis(30));

        // Now weighted should be less than limit
        let result = state.try_acquire();
        assert!(result.is_ok());
    }

    // ==================== SharedRateLimiter Tests ====================

    #[tokio::test]
    async fn test_shared_limiter_fixed() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
        );

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 1);

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }

    #[tokio::test]
    async fn test_shared_limiter_sliding_log() {
        let limiter = SharedRateLimiter::new(
            WindowType::SlidingLog,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
        );

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 1);

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }

    #[tokio::test]
    async fn test_shared_limiter_sliding_counter() {
        let limiter = SharedRateLimiter::new(
            WindowType::SlidingCounter,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
        );

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 1);

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }

    // ==================== try_acquire_no_timeout Tests ====================

    #[test]
    fn test_fixed_try_acquire_no_timeout_returns_zero_when_available() {
        let mut state =
            FixedWindowState::new(2, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_fixed_try_acquire_no_timeout_returns_wait_when_exhausted() {
        let mut state =
            FixedWindowState::new(1, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        let wait = state.try_acquire_no_timeout();
        assert!(wait > Duration::ZERO);
    }

    #[test]
    fn test_sliding_log_try_acquire_no_timeout_returns_zero_when_available() {
        let mut state = SlidingLogState::new(2, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_sliding_log_try_acquire_no_timeout_returns_wait_when_exhausted() {
        let mut state = SlidingLogState::new(1, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        let wait = state.try_acquire_no_timeout();
        assert!(wait > Duration::ZERO);
    }

    #[test]
    fn test_sliding_counter_try_acquire_no_timeout_returns_zero_when_available() {
        let mut state =
            SlidingCounterState::new(2, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_sliding_counter_try_acquire_no_timeout_returns_wait_when_exhausted() {
        let mut state =
            SlidingCounterState::new(1, Duration::from_secs(1), Duration::from_millis(100));
        assert_eq!(state.try_acquire_no_timeout(), Duration::ZERO);
        let wait = state.try_acquire_no_timeout();
        assert!(wait > Duration::ZERO);
    }

    // ==================== try_acquire_now Tests ====================

    #[test]
    fn test_try_acquire_now_ok_when_available() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
        );
        assert!(limiter.try_acquire_now().is_ok());
        assert_eq!(limiter.available_permits(), 1);
    }

    #[test]
    fn test_try_acquire_now_err_when_exhausted() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            1,
            Duration::from_secs(1),
            Duration::from_millis(100),
        );
        assert!(limiter.try_acquire_now().is_ok());
        let result = limiter.try_acquire_now();
        assert!(result.is_err());
        assert!(result.unwrap_err() > Duration::ZERO);
    }
}
