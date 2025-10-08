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
/// use tower_resilience_ratelimiter::RateLimiterConfig;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # async fn example() {
/// let rate_limiter = RateLimiterConfig::builder()
///     .limit_for_period(100)
///     .refresh_period(Duration::from_secs(1))
///     .timeout_duration(Duration::from_millis(100))
///     .build()
///     .layer();
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
}

impl<S> Layer<S> for RateLimiterLayer {
    type Service = RateLimiter<S>;

    fn layer(&self, service: S) -> Self::Service {
        RateLimiter::new(service, Arc::clone(&self.config))
    }
}
