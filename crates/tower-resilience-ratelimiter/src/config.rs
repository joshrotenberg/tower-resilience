use crate::events::RateLimiterEvent;
use std::time::Duration;
use tower_resilience_core::events::{EventListeners, FnListener};

/// The type of window used for rate limiting.
///
/// This determines how the rate limiter tracks and enforces the request limit
/// over time. Each type has different trade-offs between precision, memory usage,
/// and behavior at window boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowType {
    /// Fixed window rate limiting (default).
    ///
    /// Divides time into fixed intervals and resets the permit count at each
    /// interval boundary. Simple and memory-efficient (O(1)), but can allow
    /// up to 2x the limit in bursts at window boundaries.
    ///
    /// For example, with a limit of 100 requests per second:
    /// - 100 requests at t=0.9s (end of window 1)
    /// - 100 requests at t=1.1s (start of window 2)
    /// - Results in 200 requests in 0.2 seconds
    #[default]
    Fixed,

    /// Sliding log rate limiting.
    ///
    /// Stores the timestamp of each request and counts requests within the
    /// sliding window. Provides precise rate limiting with no burst allowance
    /// at boundaries.
    ///
    /// Trade-offs:
    /// - Memory: O(n) where n = number of requests in the window
    /// - Precision: Exact - guarantees no more than limit requests in any window
    /// - Best for: Low-to-medium throughput APIs where precision is critical
    SlidingLog,

    /// Sliding window counter rate limiting.
    ///
    /// A hybrid approach that uses weighted averaging between the current and
    /// previous time buckets. Provides approximate sliding window behavior with
    /// constant memory usage.
    ///
    /// The algorithm:
    /// 1. Divides time into buckets of `refresh_period` duration
    /// 2. Tracks request count in current and previous bucket
    /// 3. Estimates current window count as:
    ///    `previous_count * (1 - elapsed_ratio) + current_count`
    ///
    /// Trade-offs:
    /// - Memory: O(1) - only stores two counters
    /// - Precision: Approximate - slightly less accurate than SlidingLog
    /// - Best for: High-throughput APIs where memory efficiency matters
    SlidingCounter,
}

/// Configuration for the rate limiter pattern.
pub struct RateLimiterConfig {
    pub(crate) limit_for_period: usize,
    pub(crate) refresh_period: Duration,
    pub(crate) timeout_duration: Duration,
    pub(crate) window_type: WindowType,
    pub(crate) event_listeners: EventListeners<RateLimiterEvent>,
    pub(crate) name: String,
}

/// Builder for [`RateLimiterConfig`].
pub struct RateLimiterConfigBuilder {
    limit_for_period: usize,
    refresh_period: Duration,
    timeout_duration: Duration,
    window_type: WindowType,
    event_listeners: EventListeners<RateLimiterEvent>,
    name: String,
}

impl Default for RateLimiterConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiterConfigBuilder {
    /// Creates a new builder with defaults.
    ///
    /// Defaults:
    /// - limit_for_period: 50
    /// - refresh_period: 1 second
    /// - timeout_duration: 100ms
    /// - window_type: Fixed
    /// - name: `"<unnamed>"`
    pub fn new() -> Self {
        Self {
            limit_for_period: 50,
            refresh_period: Duration::from_secs(1),
            timeout_duration: Duration::from_millis(100),
            window_type: WindowType::default(),
            event_listeners: EventListeners::new(),
            name: "<unnamed>".to_string(),
        }
    }

    /// Sets the maximum number of permits available per refresh period.
    ///
    /// This is the core rate limiting parameter - for example, setting this to 100
    /// with a refresh_period of 1 second allows 100 requests per second.
    pub fn limit_for_period(mut self, limit: usize) -> Self {
        self.limit_for_period = limit;
        self
    }

    /// Sets the duration of the refresh period.
    ///
    /// After each period, the available permits are reset to limit_for_period.
    pub fn refresh_period(mut self, duration: Duration) -> Self {
        self.refresh_period = duration;
        self
    }

    /// Sets how long to wait for a permit before rejecting the request.
    ///
    /// If a permit is not available immediately, the rate limiter will wait
    /// up to this duration for the next refresh period. If the wait would
    /// exceed this timeout, the request is rejected immediately.
    pub fn timeout_duration(mut self, duration: Duration) -> Self {
        self.timeout_duration = duration;
        self
    }

    /// Sets the name for this rate limiter instance (used in events).
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the window type for rate limiting.
    ///
    /// The window type determines how the rate limiter tracks requests over time:
    ///
    /// - [`WindowType::Fixed`] (default): Resets permits at fixed intervals.
    ///   Simple and efficient but can allow bursts at window boundaries.
    ///
    /// - [`WindowType::SlidingLog`]: Tracks exact timestamps of each request.
    ///   Precise but uses O(n) memory where n = requests in window.
    ///
    /// - [`WindowType::SlidingCounter`]: Uses weighted averaging between buckets.
    ///   Approximate sliding window with O(1) memory.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};
    /// use std::time::Duration;
    ///
    /// // Use sliding log for precise rate limiting
    /// let limiter = RateLimiterLayer::builder()
    ///     .limit_for_period(100)
    ///     .refresh_period(Duration::from_secs(1))
    ///     .window_type(WindowType::SlidingLog)
    ///     .build();
    /// ```
    pub fn window_type(mut self, window_type: WindowType) -> Self {
        self.window_type = window_type;
        self
    }

    /// Registers a callback when a permit is acquired.
    ///
    /// This callback is invoked when a request successfully obtains a permit to proceed,
    /// either immediately or after waiting for the next refresh period.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the duration the request had to wait for the permit.
    /// - If the permit was immediately available, the duration will be close to zero.
    /// - If the request had to wait for the next refresh period, the duration will reflect that wait time.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let config = RateLimiterLayer::builder()
    ///     .limit_for_period(100)
    ///     .on_permit_acquired(|wait_time| {
    ///         if wait_time > Duration::from_millis(0) {
    ///             println!("Request waited {:?} for permit", wait_time);
    ///         } else {
    ///             println!("Permit immediately available");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_permit_acquired<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitAcquired { wait_duration, .. } = event {
                f(*wait_duration);
            }
        }));
        self
    }

    /// Registers a callback when a permit request is rejected.
    ///
    /// This callback is invoked when a request is rejected because it cannot obtain a permit
    /// within the configured timeout duration. This typically means the rate limiter is
    /// experiencing high load and cannot serve the request within acceptable time bounds.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the configured timeout duration that was exceeded.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let rejection_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&rejection_count);
    ///
    /// let config = RateLimiterLayer::builder()
    ///     .limit_for_period(10)
    ///     .timeout_duration(Duration::from_millis(100))
    ///     .on_permit_rejected(move |timeout| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Request rejected after {:?} timeout (total: {})", timeout, count + 1);
    ///     })
    ///     .build();
    /// ```
    pub fn on_permit_rejected<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitRejected {
                timeout_duration, ..
            } = event
            {
                f(*timeout_duration);
            }
        }));
        self
    }

    /// Registers a callback when permits are refreshed.
    ///
    /// This callback is invoked at the end of each refresh period when the available permits
    /// are reset to the configured limit. This happens periodically according to the
    /// `refresh_period` setting.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the number of permits now available after the refresh.
    /// This value will typically equal the configured `limit_for_period`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let config = RateLimiterLayer::builder()
    ///     .limit_for_period(100)
    ///     .refresh_period(Duration::from_secs(1))
    ///     .on_permits_refreshed(|available| {
    ///         println!("Rate limiter refreshed: {} permits now available", available);
    ///     })
    ///     .build();
    /// ```
    pub fn on_permits_refreshed<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitsRefreshed {
                available_permits, ..
            } = event
            {
                f(*available_permits);
            }
        }));
        self
    }

    /// Builds the rate limiter layer.
    pub fn build(self) -> crate::RateLimiterLayer {
        let config = RateLimiterConfig {
            limit_for_period: self.limit_for_period,
            refresh_period: self.refresh_period,
            timeout_duration: self.timeout_duration,
            window_type: self.window_type,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::RateLimiterLayer::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimiterLayer;

    #[test]
    fn test_builder_defaults() {
        let _layer = RateLimiterLayer::builder().build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = RateLimiterLayer::builder()
            .limit_for_period(100)
            .refresh_period(Duration::from_secs(2))
            .timeout_duration(Duration::from_millis(500))
            .name("test-limiter")
            .build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_event_listeners() {
        let _layer = RateLimiterLayer::builder()
            .on_permit_acquired(|_| {})
            .on_permit_rejected(|_| {})
            .build();
        // If this compiles and doesn't panic, the event listener registration works
    }
}
