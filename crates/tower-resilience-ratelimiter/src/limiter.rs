use crate::config::WindowType;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep, Instant};

/// The result of checking the limiter state once.
///
/// Keeping admission distinct from a zero or positive wait is important: only
/// `Acquired` means the algorithm mutated its state and consumed a permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcquireDecision {
    Acquired,
    Wait(Duration),
}

const MIN_WAIT: Duration = Duration::from_nanos(1);

fn positive_wait(duration: Duration) -> Duration {
    duration.max(MIN_WAIT)
}

/// Fixed window rate limiter state.
///
/// Resets all permits at fixed interval boundaries.
#[derive(Debug)]
struct FixedWindowState {
    limit_for_period: usize,
    refresh_period: Duration,
    available_permits: usize,
    period_start: Instant,
}

impl FixedWindowState {
    fn new(limit_for_period: usize, refresh_period: Duration) -> Self {
        Self {
            limit_for_period,
            refresh_period,
            available_permits: limit_for_period,
            period_start: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> AcquireDecision {
        let now = Instant::now();

        // Check if we need to refresh the period
        if now.duration_since(self.period_start) >= self.refresh_period {
            self.refresh(now);
        }

        // If permits available, grant immediately
        if self.available_permits > 0 {
            self.available_permits -= 1;
            return AcquireDecision::Acquired;
        }

        // No permits available - calculate wait time
        let time_until_refresh = self
            .refresh_period
            .saturating_sub(now.duration_since(self.period_start));

        AcquireDecision::Wait(positive_wait(time_until_refresh))
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
    /// Timestamps of requests within the current window.
    request_log: VecDeque<Instant>,
}

impl SlidingLogState {
    fn new(limit_for_period: usize, window_duration: Duration) -> Self {
        Self {
            limit_for_period,
            window_duration,
            request_log: VecDeque::with_capacity(limit_for_period),
        }
    }

    fn try_acquire(&mut self) -> AcquireDecision {
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
            return AcquireDecision::Acquired;
        }

        // No capacity - calculate when the oldest request will expire
        if let Some(&oldest) = self.request_log.front() {
            let time_until_slot = oldest
                .checked_add(self.window_duration)
                .map(|expiry| expiry.saturating_duration_since(now))
                .unwrap_or(Duration::ZERO);

            AcquireDecision::Wait(positive_wait(time_until_slot))
        } else {
            unreachable!("a validated non-zero limit cannot have a full empty log")
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
    /// Count of requests in the previous bucket.
    previous_count: usize,
    /// Count of requests in the current bucket.
    current_count: usize,
    /// When the current bucket started.
    bucket_start: Instant,
}

impl SlidingCounterState {
    fn new(limit_for_period: usize, bucket_duration: Duration) -> Self {
        Self {
            limit_for_period,
            bucket_duration,
            previous_count: 0,
            current_count: 0,
            bucket_start: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> AcquireDecision {
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
            return AcquireDecision::Acquired;
        }

        // No capacity - estimate when a slot will be available
        // As time progresses, previous_weight decreases, freeing up capacity
        let time_until_slot = self.estimate_wait_time(elapsed_ratio);

        AcquireDecision::Wait(positive_wait(time_until_slot))
    }

    fn maybe_rotate_bucket(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.bucket_start);

        if elapsed >= self.bucket_duration {
            if elapsed >= self.bucket_duration.saturating_mul(2) {
                // More than one full bucket passed - previous is now empty
                self.previous_count = 0;
                self.current_count = 0;
                self.bucket_start = now;
            } else {
                // Exactly one bucket passed - rotate
                self.previous_count = self.current_count;
                self.current_count = 0;

                // Preserve the actual window boundary. Assigning `now` here
                // would discard any elapsed time after the boundary and make
                // lazy rotations artificially extend the previous bucket.
                self.bucket_start += self.bucket_duration;
            }
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
            return positive_wait(Duration::from_secs_f64(remaining));
        }

        // Admission requires a strict `weighted < limit`. Solve for the point
        // where the estimate reaches the boundary, then wait one additional
        // nanosecond so a retry cannot observe equality and spin.
        let target_ratio = (previous + current - limit) / previous;

        if target_ratio <= current_ratio {
            // Already past the point where we'd have capacity
            MIN_WAIT
        } else if target_ratio >= 1.0 {
            // Need to wait for bucket rotation
            let remaining = self.bucket_duration.as_secs_f64() * (1.0 - current_ratio);
            positive_wait(Duration::from_secs_f64(remaining).saturating_add(MIN_WAIT))
        } else {
            let wait_ratio = target_ratio - current_ratio;
            positive_wait(
                Duration::from_secs_f64(wait_ratio * self.bucket_duration.as_secs_f64())
                    .saturating_add(MIN_WAIT),
            )
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

/// Continuously replenished token bucket used by the burst preset.
///
/// `limit_for_period` defines the sustained replenishment rate and
/// `burst_size` adds only storage capacity. In particular, burst credit is not
/// added again every period: after the initial full bucket is spent, new tokens
/// arrive at the sustained rate until idle time fills the bucket again.
#[derive(Debug)]
struct TokenBucketState {
    limit_for_period: usize,
    refresh_period: Duration,
    capacity: usize,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucketState {
    fn new(limit_for_period: usize, refresh_period: Duration, burst_size: usize) -> Self {
        let capacity = limit_for_period
            .checked_add(burst_size)
            .expect("limit_for_period + burst_size must not overflow");

        Self {
            limit_for_period,
            refresh_period,
            capacity,
            tokens: capacity as f64,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> AcquireDecision {
        let now = Instant::now();
        self.refill(now);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return AcquireDecision::Acquired;
        }

        let missing_tokens = 1.0 - self.tokens;
        let seconds_per_token = self.refresh_period.as_secs_f64() / self.limit_for_period as f64;
        AcquireDecision::Wait(positive_wait(Duration::from_secs_f64(
            missing_tokens * seconds_per_token,
        )))
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill);
        if elapsed.is_zero() {
            return;
        }

        let replenished = elapsed.as_secs_f64() * self.limit_for_period as f64
            / self.refresh_period.as_secs_f64();
        self.tokens = (self.tokens + replenished).min(self.capacity as f64);
        self.last_refill = now;
    }

    fn available_permits(&self) -> usize {
        let elapsed = Instant::now().duration_since(self.last_refill);
        let replenished = elapsed.as_secs_f64() * self.limit_for_period as f64
            / self.refresh_period.as_secs_f64();
        (self.tokens + replenished)
            .min(self.capacity as f64)
            .floor() as usize
    }
}

/// Enum-based rate limiter state that dispatches to the appropriate implementation.
#[derive(Debug)]
enum RateLimiterStateInner {
    Fixed(FixedWindowState),
    SlidingLog(SlidingLogState),
    SlidingCounter(SlidingCounterState),
    TokenBucket(TokenBucketState),
}

impl RateLimiterStateInner {
    fn new(
        window_type: WindowType,
        limit_for_period: usize,
        refresh_period: Duration,
        burst_size: Option<usize>,
    ) -> Self {
        if let Some(burst_size) = burst_size {
            return Self::TokenBucket(TokenBucketState::new(
                limit_for_period,
                refresh_period,
                burst_size,
            ));
        }

        match window_type {
            WindowType::Fixed => {
                Self::Fixed(FixedWindowState::new(limit_for_period, refresh_period))
            }
            WindowType::SlidingLog => {
                Self::SlidingLog(SlidingLogState::new(limit_for_period, refresh_period))
            }
            WindowType::SlidingCounter => {
                Self::SlidingCounter(SlidingCounterState::new(limit_for_period, refresh_period))
            }
        }
    }

    fn try_acquire(&mut self) -> AcquireDecision {
        match self {
            Self::Fixed(state) => state.try_acquire(),
            Self::SlidingLog(state) => state.try_acquire(),
            Self::SlidingCounter(state) => state.try_acquire(),
            Self::TokenBucket(state) => state.try_acquire(),
        }
    }

    fn available_permits(&self) -> usize {
        match self {
            Self::Fixed(state) => state.available_permits(),
            Self::SlidingLog(state) => state.available_permits(),
            Self::SlidingCounter(state) => state.available_permits(),
            Self::TokenBucket(state) => state.available_permits(),
        }
    }
}

/// Shared rate limiter that can be cloned across services.
#[derive(Debug, Clone)]
pub(crate) struct SharedRateLimiter {
    state: Arc<Mutex<RateLimiterStateInner>>,
    timeout_duration: Duration,
}

impl SharedRateLimiter {
    pub(crate) fn new(
        window_type: WindowType,
        limit_for_period: usize,
        refresh_period: Duration,
        timeout_duration: Duration,
        burst_size: Option<usize>,
    ) -> Self {
        assert!(
            limit_for_period > 0,
            "limit_for_period must be greater than zero"
        );
        assert!(
            !refresh_period.is_zero(),
            "refresh_period must be greater than zero"
        );
        if let Some(burst_size) = burst_size {
            limit_for_period
                .checked_add(burst_size)
                .expect("limit_for_period + burst_size must not overflow");
        }

        Self {
            state: Arc::new(Mutex::new(RateLimiterStateInner::new(
                window_type,
                limit_for_period,
                refresh_period,
                burst_size,
            ))),
            timeout_duration,
        }
    }

    /// Attempts to acquire a permit.
    /// Returns Ok(duration_waited) if successful, Err if rate limited.
    pub(crate) async fn acquire(&self) -> Result<Duration, ()> {
        let started_at = Instant::now();
        let mut first_attempt = true;

        loop {
            // Always allow the initial immediate attempt, including when the
            // configured timeout is zero. After a wait, enforce one total
            // timeout across every retry rather than restarting it.
            if !first_attempt && started_at.elapsed() >= self.timeout_duration {
                return Err(());
            }

            let decision = {
                let mut state = self.state.lock().unwrap();
                state.try_acquire()
            };

            match decision {
                AcquireDecision::Acquired => {
                    let waited = if first_attempt {
                        Duration::ZERO
                    } else {
                        started_at.elapsed()
                    };
                    return Ok(waited);
                }
                AcquireDecision::Wait(wait_duration) => {
                    let remaining = self.timeout_duration.saturating_sub(started_at.elapsed());
                    if remaining.is_zero() {
                        return Err(());
                    }

                    // No state or permit is held while sleeping. Dropping this
                    // future simply cancels the timer and cannot admit a call.
                    sleep(wait_duration.min(remaining)).await;
                    first_attempt = false;
                }
            }
        }
    }

    /// Attempts to acquire a permit immediately without waiting or timeout.
    ///
    /// Returns `Ok(())` if a permit was consumed, or `Err(wait_duration)` indicating
    /// how long to wait before retrying.
    pub(crate) fn try_acquire_now(&self) -> Result<(), Duration> {
        let mut state = self.state.lock().unwrap();
        match state.try_acquire() {
            AcquireDecision::Acquired => Ok(()),
            AcquireDecision::Wait(wait_duration) => Err(wait_duration),
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

    fn assert_acquired(decision: AcquireDecision) {
        assert_eq!(decision, AcquireDecision::Acquired);
    }

    fn wait_duration(decision: AcquireDecision) -> Duration {
        match decision {
            AcquireDecision::Wait(duration) => duration,
            AcquireDecision::Acquired => panic!("expected the limiter to require a wait"),
        }
    }

    // ==================== Fixed Window Tests ====================

    #[test]
    fn test_fixed_initial_permits() {
        let state = FixedWindowState::new(10, Duration::from_secs(1));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_fixed_acquire_permit() {
        let mut state = FixedWindowState::new(10, Duration::from_secs(1));

        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_fixed_exhaust_permits() {
        let mut state = FixedWindowState::new(2, Duration::from_millis(100));

        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    #[test]
    fn test_fixed_refresh_restores_permits() {
        let mut state = FixedWindowState::new(5, Duration::from_millis(10));

        for _ in 0..5 {
            assert_acquired(state.try_acquire());
        }
        assert_eq!(state.available_permits(), 0);

        std::thread::sleep(Duration::from_millis(15));

        assert_acquired(state.try_acquire());
        assert!(state.available_permits() > 0);
    }

    // ==================== Sliding Log Tests ====================

    #[test]
    fn test_sliding_log_initial_permits() {
        let state = SlidingLogState::new(10, Duration::from_secs(1));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_sliding_log_acquire_permit() {
        let mut state = SlidingLogState::new(10, Duration::from_secs(1));

        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_sliding_log_exhaust_permits() {
        let mut state = SlidingLogState::new(2, Duration::from_millis(100));

        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    #[test]
    fn test_sliding_log_expires_old_requests() {
        let mut state = SlidingLogState::new(2, Duration::from_millis(50));

        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 0);

        // Wait for requests to expire
        std::thread::sleep(Duration::from_millis(60));

        // Should be able to acquire again
        assert_acquired(state.try_acquire());
    }

    #[tokio::test(start_paused = true)]
    async fn test_sliding_log_no_boundary_burst() {
        let mut state = SlidingLogState::new(2, Duration::from_millis(100));

        // Acquire 2 permits
        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());

        // Advance 60ms (past a hypothetical 50ms fixed-window boundary but
        // still within the sliding log's 100ms window). A paused clock keeps
        // concurrent test scheduling from expiring the entries accidentally.
        tokio::time::advance(Duration::from_millis(60)).await;

        // With sliding log, these requests are still in the window
        // so we should NOT be able to acquire more (unlike fixed window)
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    // ==================== Sliding Counter Tests ====================

    #[test]
    fn test_sliding_counter_initial_permits() {
        let state = SlidingCounterState::new(10, Duration::from_secs(1));
        assert_eq!(state.available_permits(), 10);
    }

    #[test]
    fn test_sliding_counter_acquire_permit() {
        let mut state = SlidingCounterState::new(10, Duration::from_secs(1));

        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 9);
    }

    #[test]
    fn test_sliding_counter_exhaust_permits() {
        let mut state = SlidingCounterState::new(2, Duration::from_millis(100));

        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 0);

        // Next acquire should indicate wait needed
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    #[test]
    fn test_sliding_counter_bucket_rotation() {
        let mut state = SlidingCounterState::new(2, Duration::from_millis(50));

        assert_acquired(state.try_acquire());
        assert_acquired(state.try_acquire());

        // Wait for bucket to rotate
        std::thread::sleep(Duration::from_millis(55));

        // After rotation, previous_count = 2, current_count = 0
        // At start of new bucket, weighted = 2 * 1.0 + 0 = 2, so still at limit
        // But as time progresses, previous weight decreases

        // Wait a bit more for previous contribution to decrease
        std::thread::sleep(Duration::from_millis(30));

        // Now weighted should be less than limit
        assert_acquired(state.try_acquire());
    }

    // ==================== SharedRateLimiter Tests ====================

    #[tokio::test]
    async fn test_shared_limiter_fixed() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
            None,
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
            None,
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
            None,
        );

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 1);

        assert!(limiter.acquire().await.is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }

    // ==================== AcquireDecision Tests ====================

    #[test]
    fn test_fixed_decision_is_acquired_when_available() {
        let mut state = FixedWindowState::new(2, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_fixed_decision_is_wait_when_exhausted() {
        let mut state = FixedWindowState::new(1, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    #[test]
    fn test_sliding_log_decision_is_acquired_when_available() {
        let mut state = SlidingLogState::new(2, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_sliding_log_decision_is_wait_when_exhausted() {
        let mut state = SlidingLogState::new(1, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    #[test]
    fn test_sliding_counter_decision_is_acquired_when_available() {
        let mut state = SlidingCounterState::new(2, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert_eq!(state.available_permits(), 1);
    }

    #[test]
    fn test_sliding_counter_decision_is_wait_when_exhausted() {
        let mut state = SlidingCounterState::new(1, Duration::from_secs(1));
        assert_acquired(state.try_acquire());
        assert!(wait_duration(state.try_acquire()) > Duration::ZERO);
    }

    // ==================== try_acquire_now Tests ====================

    #[test]
    fn test_try_acquire_now_ok_when_available() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            2,
            Duration::from_secs(1),
            Duration::from_millis(100),
            None,
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
            None,
        );
        assert!(limiter.try_acquire_now().is_ok());
        let result = limiter.try_acquire_now();
        assert!(result.is_err());
        assert!(result.unwrap_err() > Duration::ZERO);
    }

    async fn contend(window_type: WindowType) -> Vec<Duration> {
        use futures::future::join_all;
        use tokio::sync::Barrier;

        const LIMIT: usize = 3;
        const CALLERS: usize = 10;

        let limiter = SharedRateLimiter::new(
            window_type,
            LIMIT,
            Duration::from_secs(1),
            Duration::from_secs(6),
            None,
        );
        let barrier = Arc::new(Barrier::new(CALLERS + 1));
        let started_at = Instant::now();

        let tasks = (0..CALLERS)
            .map(|_| {
                let limiter = limiter.clone();
                let barrier = Arc::clone(&barrier);
                tokio::spawn(async move {
                    barrier.wait().await;
                    limiter.acquire().await?;
                    Ok::<_, ()>(Instant::now().duration_since(started_at))
                })
            })
            .collect::<Vec<_>>();

        barrier.wait().await;
        let mut admitted = join_all(tasks)
            .await
            .into_iter()
            .map(|result| {
                result
                    .expect("contention task panicked")
                    .expect("timed out")
            })
            .collect::<Vec<_>>();
        admitted.sort_unstable();
        admitted
    }

    fn assert_no_thundering_herd(admitted: &[Duration], limit: usize) {
        let mut group_start = 0;
        while group_start < admitted.len() {
            let timestamp = admitted[group_start];
            let group_end = admitted[group_start..]
                .iter()
                .position(|candidate| *candidate != timestamp)
                .map_or(admitted.len(), |offset| group_start + offset);
            assert!(
                group_end - group_start <= limit,
                "{} callers were admitted together at {timestamp:?}, limit was {limit}",
                group_end - group_start
            );
            group_start = group_end;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fixed_window_contention_consumes_one_permit_per_admission() {
        let admitted = contend(WindowType::Fixed).await;
        assert_eq!(admitted.len(), 10);
        assert_no_thundering_herd(&admitted, 3);

        for second in 0..=3 {
            let count = admitted
                .iter()
                .filter(|timestamp| timestamp.as_secs() == second)
                .count();
            assert!(count <= 3, "fixed window {second} admitted {count}");
        }
        assert!(admitted.last().copied().unwrap() >= Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn sliding_log_contention_consumes_one_permit_per_admission() {
        let admitted = contend(WindowType::SlidingLog).await;
        assert_eq!(admitted.len(), 10);
        assert_no_thundering_herd(&admitted, 3);

        for (index, timestamp) in admitted.iter().enumerate() {
            let window_start = timestamp.saturating_sub(Duration::from_secs(1));
            let in_window = admitted[..=index]
                .iter()
                .filter(|candidate| **candidate > window_start)
                .count();
            assert!(
                in_window <= 3,
                "sliding window ending at {timestamp:?} admitted {in_window}"
            );
        }
        assert!(admitted.last().copied().unwrap() >= Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn sliding_counter_contention_retries_until_permit_is_consumed() {
        let admitted = contend(WindowType::SlidingCounter).await;
        assert_eq!(admitted.len(), 10);
        assert_no_thundering_herd(&admitted, 3);

        for second in 0..=3 {
            let count = admitted
                .iter()
                .filter(|timestamp| timestamp.as_secs() == second)
                .count();
            assert!(count <= 3, "counter window {second} admitted {count}");
        }
        assert!(admitted.last().copied().unwrap() > Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn contention_uses_one_total_timeout_across_retries() {
        use futures::future::join_all;
        use tokio::sync::Barrier;

        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            1,
            Duration::from_secs(1),
            Duration::from_millis(1500),
            None,
        );
        assert!(limiter.acquire().await.is_ok());

        let barrier = Arc::new(Barrier::new(3));
        let started_at = Instant::now();
        let tasks = (0..2)
            .map(|_| {
                let limiter = limiter.clone();
                let barrier = Arc::clone(&barrier);
                tokio::spawn(async move {
                    barrier.wait().await;
                    (limiter.acquire().await, started_at.elapsed())
                })
            })
            .collect::<Vec<_>>();

        barrier.wait().await;
        let results = join_all(tasks)
            .await
            .into_iter()
            .map(|result| result.expect("contention task panicked"))
            .collect::<Vec<_>>();

        assert_eq!(
            results.iter().filter(|(result, _)| result.is_ok()).count(),
            1
        );
        let rejected_at = results
            .iter()
            .find_map(|(result, elapsed)| result.is_err().then_some(*elapsed))
            .expect("one waiter must time out");
        assert_eq!(rejected_at, Duration::from_millis(1500));
        assert_eq!(limiter.available_permits(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_waiter_does_not_consume_a_future_permit() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            1,
            Duration::from_secs(1),
            Duration::from_secs(5),
            None,
        );
        assert!(limiter.acquire().await.is_ok());

        let waiting = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire().await })
        };
        tokio::task::yield_now().await;
        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());

        sleep(Duration::from_secs(1)).await;
        assert!(limiter.try_acquire_now().is_ok());
        assert_eq!(limiter.available_permits(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn token_bucket_replenishes_sustained_rate_not_burst_capacity() {
        let limiter = SharedRateLimiter::new(
            WindowType::Fixed,
            2,
            Duration::from_secs(1),
            Duration::from_secs(3),
            Some(2),
        );

        // An idle bucket starts with sustained allotment plus burst credit.
        for _ in 0..4 {
            assert_eq!(limiter.acquire().await.unwrap(), Duration::ZERO);
        }
        assert_eq!(limiter.available_permits(), 0);

        // Credit is restored at two tokens/second, not four tokens/second.
        let waited = limiter.acquire().await.unwrap();
        assert_eq!(waited, Duration::from_millis(500));
        assert_eq!(limiter.available_permits(), 0);

        sleep(Duration::from_secs(1)).await;
        assert_eq!(limiter.available_permits(), 2);

        // Idle replenishment is capped at sustained allotment + burst credit.
        sleep(Duration::from_secs(10)).await;
        assert_eq!(limiter.available_permits(), 4);
    }
}
