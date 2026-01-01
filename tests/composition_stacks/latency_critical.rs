//! Latency-critical path stack examples.
//!
//! These stacks are designed for operations where P99 latency matters
//! (trading systems, real-time applications, etc.)
//!
//! Note: Types are prefixed with `Latency` to avoid confusion with
//! the `caching` module's types which serve a different purpose.

use std::time::Duration;

use tower::{Layer, Service};
use tower_resilience_hedge::HedgeLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for latency-critical operations
#[derive(Debug, Clone)]
struct LatencyCacheError(String);

impl std::fmt::Display for LatencyCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LatencyCacheError: {}", self.0)
    }
}

impl std::error::Error for LatencyCacheError {}

/// Test request type for latency-critical lookups
#[derive(Debug, Clone)]
struct LatencyCacheKey(String);

/// Test response type for latency-critical lookups
#[derive(Debug, Clone)]
struct LatencyCacheValue(Vec<u8>);

/// Creates a mock cache client service for latency-critical operations
fn mock_cache_client()
-> impl Service<LatencyCacheKey, Response = LatencyCacheValue, Error = LatencyCacheError> + Clone {
    tower::service_fn(|key: LatencyCacheKey| async move {
        Ok(LatencyCacheValue(
            format!("value for {}", key.0).into_bytes(),
        ))
    })
}

/// Creates a mock multi-region client service
fn mock_multi_region_client()
-> impl Service<LatencyCacheKey, Response = LatencyCacheValue, Error = LatencyCacheError> + Clone {
    tower::service_fn(|key: LatencyCacheKey| async move {
        Ok(LatencyCacheValue(
            format!("multi-region value for {}", key.0).into_bytes(),
        ))
    })
}

/// Latency mode hedging: fire hedge after delay
#[tokio::test]
async fn latency_mode_hedging_compiles() {
    let hedge = HedgeLayer::<LatencyCacheKey, LatencyCacheValue, LatencyCacheError>::builder()
        .delay(Duration::from_millis(10)) // Fire hedge after 10ms
        .max_hedged_attempts(2)
        .build();

    let timeout = TimeLimiterLayer::<LatencyCacheKey>::builder()
        .timeout_duration(Duration::from_millis(100)) // Tight deadline
        .build();

    let cache_client = mock_cache_client();

    // Manual composition
    let with_hedge = hedge.layer(cache_client);
    let _service = timeout.layer(with_hedge);
}

/// Parallel mode hedging: fire all requests immediately
#[tokio::test]
async fn parallel_mode_hedging_compiles() {
    let hedge = HedgeLayer::<LatencyCacheKey, LatencyCacheValue, LatencyCacheError>::builder()
        .no_delay() // Fire all requests immediately
        .max_hedged_attempts(3)
        .build();

    let timeout = TimeLimiterLayer::<LatencyCacheKey>::builder()
        .timeout_duration(Duration::from_millis(50)) // Very tight deadline
        .build();

    let multi_region_client = mock_multi_region_client();

    // Manual composition
    let with_hedge = hedge.layer(multi_region_client);
    let _service = timeout.layer(with_hedge);
}

/// Dynamic delay hedging
#[tokio::test]
async fn dynamic_delay_hedging_compiles() {
    let hedge = HedgeLayer::<LatencyCacheKey, LatencyCacheValue, LatencyCacheError>::builder()
        .delay_fn(|attempt| Duration::from_millis(10 * (attempt as u64).pow(2)))
        .max_hedged_attempts(3)
        .build();

    let timeout = TimeLimiterLayer::<LatencyCacheKey>::builder()
        .timeout_duration(Duration::from_millis(200))
        .build();

    let cache_client = mock_cache_client();

    let with_hedge = hedge.layer(cache_client);
    let _service = timeout.layer(with_hedge);
}
