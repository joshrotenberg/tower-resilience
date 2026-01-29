use crate::{RateLimiter, RateLimiterConfig};
use std::sync::Arc;
use tower::Layer;

/// A Tower [`Layer`] that applies rate limiting to a service.
///
/// This layer wraps a service with a rate limiter that controls the rate
/// of requests based on the configured limit per period.
///
/// # Examples
///
/// ```
/// use tower_resilience_ratelimiter::RateLimiterLayer;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # async fn example() {
/// let rate_limiter = RateLimiterLayer::builder()
///     .limit_for_period(100)
///     .refresh_period(Duration::from_secs(1))
///     .timeout_duration(Duration::from_millis(100))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(rate_limiter)
///     .service(my_service());
/// # }
/// # fn my_service() -> impl tower::Service<String, Response = String, Error = std::io::Error> {
/// #     tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) })
/// # }
/// ```
#[derive(Clone)]
pub struct RateLimiterLayer {
    config: Arc<RateLimiterConfig>,
}

impl RateLimiterLayer {
    /// Creates a new `RateLimiterLayer` with the given configuration.
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Creates a new builder for configuring a rate limiter layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let layer = RateLimiterLayer::builder()
    ///     .limit_for_period(100)
    ///     .refresh_period(Duration::from_secs(1))
    ///     .build();
    /// ```
    pub fn builder() -> crate::RateLimiterConfigBuilder {
        crate::RateLimiterConfigBuilder::new()
    }

    // =========================================================================
    // Presets
    // =========================================================================

    /// Preset: Rate limit of N requests per second.
    ///
    /// Configuration:
    /// - `limit` requests per 1 second period
    /// - 100ms timeout waiting for permits
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    ///
    /// // Allow 100 requests per second
    /// let layer = RateLimiterLayer::per_second(100).build();
    ///
    /// // Customize further
    /// let layer = RateLimiterLayer::per_second(100)
    ///     .timeout_duration(std::time::Duration::from_millis(500))
    ///     .build();
    /// ```
    pub fn per_second(limit: usize) -> crate::RateLimiterConfigBuilder {
        use std::time::Duration;
        Self::builder()
            .limit_for_period(limit)
            .refresh_period(Duration::from_secs(1))
            .timeout_duration(Duration::from_millis(100))
    }

    /// Preset: Rate limit of N requests per minute.
    ///
    /// Configuration:
    /// - `limit` requests per 60 second period
    /// - 1 second timeout waiting for permits
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    ///
    /// // Allow 1000 requests per minute
    /// let layer = RateLimiterLayer::per_minute(1000).build();
    /// ```
    pub fn per_minute(limit: usize) -> crate::RateLimiterConfigBuilder {
        use std::time::Duration;
        Self::builder()
            .limit_for_period(limit)
            .refresh_period(Duration::from_secs(60))
            .timeout_duration(Duration::from_secs(1))
    }

    /// Preset: Rate limit with burst capacity.
    ///
    /// Configuration:
    /// - `rate_per_second` sustained requests per second
    /// - `burst_size` additional burst capacity above the sustained rate
    /// - 100ms timeout waiting for permits
    ///
    /// This uses a sliding window to smooth out the rate limiting while
    /// allowing short bursts of traffic.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_ratelimiter::RateLimiterLayer;
    ///
    /// // Allow 100 requests/sec sustained with burst up to 150
    /// let layer = RateLimiterLayer::burst(100, 50).build();
    /// ```
    pub fn burst(rate_per_second: usize, burst_size: usize) -> crate::RateLimiterConfigBuilder {
        use std::time::Duration;
        Self::builder()
            .limit_for_period(rate_per_second + burst_size)
            .refresh_period(Duration::from_secs(1))
            .timeout_duration(Duration::from_millis(100))
            .window_type(crate::WindowType::SlidingCounter)
    }
}

impl<S> Layer<S> for RateLimiterLayer {
    type Service = RateLimiter<S>;

    fn layer(&self, service: S) -> Self::Service {
        RateLimiter::new(service, Arc::clone(&self.config))
    }
}
