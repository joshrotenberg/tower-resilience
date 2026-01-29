//! Caching layer stack examples.
//!
//! These stacks are designed for Redis, Memcached, etc.

use std::time::Duration;

use tower::{Layer, Service};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_coalesce::CoalesceLayer;
use tower_resilience_fallback::FallbackLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for cache operations
#[derive(Debug, Clone)]
struct RedisError(String);

impl std::fmt::Display for RedisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedisError: {}", self.0)
    }
}

impl std::error::Error for RedisError {}

/// Cache key
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey(String);

/// Cache value (Option to represent cache miss)
#[derive(Debug, Clone)]
struct CacheValue(Option<Vec<u8>>);

/// Creates a mock Redis client service
fn mock_redis_client() -> impl Service<CacheKey, Response = CacheValue, Error = RedisError> + Clone
{
    tower::service_fn(|key: CacheKey| async move {
        Ok(CacheValue(Some(
            format!("cached value for {}", key.0).into_bytes(),
        )))
    })
}

/// Standard cache stack: Fallback (cache miss OK) + Timeout + CircuitBreaker
#[tokio::test]
async fn standard_cache_stack_compiles() {
    // Fallback to None (cache miss) on error - outermost to catch all errors
    let fallback = FallbackLayer::<CacheKey, CacheValue, RedisError>::value(CacheValue(None));

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.3) // Sensitive threshold for cache
        .build();

    let timeout = TimeLimiterLayer::<CacheKey>::builder()
        .timeout_duration(Duration::from_millis(50)) // Fast timeout for cache
        .build();

    let redis_client = mock_redis_client();

    // Manual composition (innermost to outermost)
    // ServiceBuilder order: Fallback -> Timeout -> CircuitBreaker -> service
    // Manual order: apply CircuitBreaker first, then Timeout, then Fallback
    let with_cb = circuit_breaker.layer(redis_client);
    let with_timeout = timeout.layer(with_cb);
    let _service = fallback.layer(with_timeout);
}

/// Cache stack with request coalescing (simpler - coalesce + timeout)
#[tokio::test]
async fn cache_with_coalescing_compiles() {
    let coalesce = CoalesceLayer::new(|req: &CacheKey| req.clone());

    let timeout = TimeLimiterLayer::<CacheKey>::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let redis_client = mock_redis_client();

    // Manual composition - coalesce dedupes concurrent identical requests
    let with_coalesce = coalesce.layer(redis_client);
    let _service = timeout.layer(with_coalesce);
}

/// Cache with fallback from error (dynamic fallback)
#[tokio::test]
async fn cache_with_dynamic_fallback_compiles() {
    let fallback = FallbackLayer::<CacheKey, CacheValue, RedisError>::from_error(|_e| {
        // Return empty cache value on any error
        CacheValue(None)
    });

    let timeout = TimeLimiterLayer::<CacheKey>::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let redis_client = mock_redis_client();

    let with_fallback = fallback.layer(redis_client);
    let _service = timeout.layer(with_fallback);
}
