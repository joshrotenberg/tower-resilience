# tower-resilience

[![Crates.io](https://img.shields.io/crates/v/tower-resilience.svg)](https://crates.io/crates/tower-resilience)
[![Documentation](https://docs.rs/tower-resilience/badge.svg)](https://docs.rs/tower-resilience)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.64.0%2B-blue.svg)](https://www.rust-lang.org)

A comprehensive resilience and fault-tolerance toolkit for [Tower](https://github.com/tower-rs/tower) services, inspired by [Resilience4j](https://resilience4j.readme.io/).

## About

Resilience patterns for [Tower](https://docs.rs/tower) services, inspired by [Resilience4j](https://resilience4j.readme.io/). Includes circuit breaker, bulkhead, retry with backoff, rate limiting, and more.

## Resilience Patterns

- **Circuit Breaker** - Prevents cascading failures by stopping calls to failing services
- **Bulkhead** - Isolates resources to prevent system-wide failures  
- **Time Limiter** - Advanced timeout handling with cancellation support
- **Retry** - Intelligent retry with exponential backoff, jitter, and retry budgets
- **Rate Limiter** - Controls request rate with fixed or sliding window algorithms
- **Cache** - Response memoization to reduce load
- **Fallback** - Graceful degradation when services fail
- **Hedge** - Reduces tail latency by racing redundant requests
- **Reconnect** - Automatic reconnection with configurable backoff strategies
- **Health Check** - Proactive health monitoring with intelligent resource selection
- **Executor** - Delegates request processing to dedicated executors for parallelism
- **Adaptive Concurrency** - Dynamic concurrency limiting using AIMD or Vegas algorithms
- **Coalesce** - Deduplicates concurrent identical requests (singleflight pattern)
- **Chaos** - Inject failures and latency for testing resilience (development/testing only)

## Quick Start

```toml
[dependencies]
tower-resilience = "0.7"
tower = "0.5"
```

```rust
use tower::{Layer, ServiceBuilder};
use tower_resilience::prelude::*;

let circuit_breaker = CircuitBreakerLayer::builder()
    .failure_rate_threshold(0.5)
    .build();

let service = ServiceBuilder::new()
    .layer(circuit_breaker)
    .layer(BulkheadLayer::builder()
        .max_concurrent_calls(10)
        .build())
    .service(my_service);
```

## Presets: Get Started in One Line

Every pattern includes **preset configurations** with sensible defaults. Start immediately without tuning parameters - customize later when you need to:

```rust
use tower_resilience_retry::RetryLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_bulkhead::BulkheadLayer;

// Retry with exponential backoff (3 attempts, 100ms base)
let retry = RetryLayer::<(), MyError>::exponential_backoff().build();

// Circuit breaker with balanced defaults
let breaker = CircuitBreakerLayer::standard().build();

// Rate limit to 100 requests per second
let limiter = RateLimiterLayer::per_second(100).build();

// Limit to 50 concurrent requests
let bulkhead = BulkheadLayer::medium().build();
```

### Available Presets

| Pattern | Presets | Description |
|---------|---------|-------------|
| **Retry** | `exponential_backoff()` | 3 attempts, 100ms base - balanced default |
| | `aggressive()` | 5 attempts, 50ms base - fast recovery |
| | `conservative()` | 2 attempts, 500ms base - minimal overhead |
| **Circuit Breaker** | `standard()` | 50% threshold, 100 calls - balanced |
| | `fast_fail()` | 25% threshold, 20 calls - fail fast |
| | `tolerant()` | 75% threshold, 200 calls - high tolerance |
| **Rate Limiter** | `per_second(n)` | n requests per second |
| | `per_minute(n)` | n requests per minute |
| | `burst(rate, size)` | Sustained rate with burst capacity |
| **Bulkhead** | `small()` | 10 concurrent calls |
| | `medium()` | 50 concurrent calls |
| | `large()` | 200 concurrent calls |

Presets return builders, so you can customize any setting:

```rust
// Start with a preset, override what you need
let breaker = CircuitBreakerLayer::fast_fail()
    .name("payment-api")           // Add observability
    .wait_duration_in_open(Duration::from_secs(30))  // Custom recovery time
    .build();
```

## Examples

### Circuit Breaker

Prevent cascading failures by opening the circuit when error rate exceeds threshold:

```rust
use tower::Layer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use std::time::Duration;

let layer = CircuitBreakerLayer::builder()
    .name("api-circuit")
    .failure_rate_threshold(0.5)          // Open at 50% failure rate
    .sliding_window_size(100)              // Track last 100 calls
    .wait_duration_in_open(Duration::from_secs(60))  // Stay open 60s
    .on_state_transition(|from, to| {
        println!("Circuit breaker: {:?} -> {:?}", from, to);
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [circuitbreaker.rs](examples/circuitbreaker.rs) | [circuitbreaker_fallback.rs](crates/tower-resilience-circuitbreaker/examples/circuitbreaker_fallback.rs) | [circuitbreaker_health_check.rs](crates/tower-resilience-circuitbreaker/examples/circuitbreaker_health_check.rs)

### Bulkhead

Limit concurrent requests to prevent resource exhaustion:

```rust
use tower_resilience_bulkhead::BulkheadLayer;
use std::time::Duration;

let layer = BulkheadLayer::builder()
    .name("worker-pool")
    .max_concurrent_calls(10)                    // Max 10 concurrent
    .max_wait_duration(Duration::from_secs(5))        // Wait up to 5s
    .on_call_permitted(|concurrent| {
        println!("Request permitted (concurrent: {})", concurrent);
    })
    .on_call_rejected(|max| {
        println!("Request rejected (max: {})", max);
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [bulkhead.rs](examples/bulkhead.rs) | [bulkhead_advanced.rs](crates/tower-resilience-bulkhead/examples/bulkhead_advanced.rs)

### Time Limiter

Enforce timeouts on operations with configurable cancellation:

```rust
use tower_resilience_timelimiter::TimeLimiterLayer;
use std::time::Duration;

let layer = TimeLimiterLayer::builder()
    .timeout_duration(Duration::from_secs(30))
    .cancel_running_future(true)  // Cancel on timeout
    .on_timeout(|| {
        println!("Operation timed out!");
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [timelimiter.rs](examples/timelimiter.rs) | [timelimiter_example.rs](crates/tower-resilience-timelimiter/examples/timelimiter_example.rs)

### Retry

Retry failed requests with exponential backoff and jitter:

```rust
use tower_resilience_retry::RetryLayer;
use std::time::Duration;

let layer = RetryLayer::<(), MyError>::builder()
    .max_attempts(5)
    .exponential_backoff(Duration::from_millis(100))
    .on_retry(|attempt, delay| {
        println!("Retrying (attempt {}, delay {:?})", attempt, delay);
    })
    .on_success(|attempts| {
        println!("Success after {} attempts", attempts);
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [retry.rs](examples/retry.rs) | [retry_example.rs](crates/tower-resilience-retry/examples/retry_example.rs)

### Rate Limiter

Control request rate to protect downstream services:

```rust
use tower_resilience_ratelimiter::RateLimiterLayer;
use std::time::Duration;

let layer = RateLimiterLayer::builder()
    .limit_for_period(100)                      // 100 requests
    .refresh_period(Duration::from_secs(1))     // per second
    .timeout_duration(Duration::from_millis(500))  // Wait up to 500ms
    .on_permit_acquired(|wait| {
        println!("Request permitted (waited {:?})", wait);
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [ratelimiter.rs](examples/ratelimiter.rs) | [ratelimiter_example.rs](crates/tower-resilience-ratelimiter/examples/ratelimiter_example.rs)

### Cache

Cache responses to reduce load on expensive operations:

```rust
use tower_resilience_cache::{CacheLayer, EvictionPolicy};
use std::time::Duration;

let layer = CacheLayer::builder()
    .max_size(1000)
    .ttl(Duration::from_secs(300))                 // 5 minute TTL
    .eviction_policy(EvictionPolicy::Lru)          // LRU, LFU, or FIFO
    .key_extractor(|req: &Request| req.id.clone())
    .on_hit(|| println!("Cache hit!"))
    .on_miss(|| println!("Cache miss"))
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [cache.rs](examples/cache.rs) | [cache_example.rs](crates/tower-resilience-cache/examples/cache_example.rs)

### Fallback

Provide fallback responses when the primary service fails:

```rust
use tower_resilience_fallback::FallbackLayer;

// Return a static fallback value on error
let layer = FallbackLayer::<Request, Response, MyError>::value(
    Response::default()
);

// Or compute fallback from the error
let layer = FallbackLayer::<Request, Response, MyError>::from_error(|err| {
    Response::error_response(err)
});

// Or use a backup service
let layer = FallbackLayer::<Request, Response, MyError>::service(|req| async {
    backup_service.call(req).await
});

let service = layer.layer(primary_service);
```

### Hedge

Reduce tail latency by firing backup requests after a delay:

```rust
use tower_resilience_hedge::HedgeLayer;
use std::time::Duration;

// Fire a hedge request if primary takes > 100ms
let layer = HedgeLayer::builder()
    .delay(Duration::from_millis(100))
    .max_hedged_attempts(2)
    .build();

// Or fire all requests in parallel (no delay)
let layer = HedgeLayer::<(), String, MyError>::builder()
    .no_delay()
    .max_hedged_attempts(3)
    .build();

let service = layer.layer(my_service);
```

**Note:** Hedge requires `Req: Clone` (requests are cloned for parallel execution) and `E: Clone` (for error handling). If your types don't implement Clone, consider wrapping them in `Arc`.

**Full examples:** [hedge.rs](examples/hedge.rs)

### Reconnect

Automatically reconnect on connection failures with configurable backoff:

```rust
use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
use std::time::Duration;

let layer = ReconnectLayer::new(
    ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100),  // Start at 100ms
            Duration::from_secs(5),       // Max 5 seconds
        ))
        .max_attempts(10)
        .retry_on_reconnect(true)         // Retry request after reconnecting
        .connection_errors_only()          // Only reconnect on connection errors
        .on_state_change(|from, to| {
            println!("Connection: {:?} -> {:?}", from, to);
        })
        .build()
);

let service = layer.layer(my_service);
```

**Full examples:** [reconnect.rs](examples/reconnect.rs) | [reconnect_basic.rs](crates/tower-resilience-reconnect/examples/reconnect_basic.rs) | [reconnect_custom_policy.rs](crates/tower-resilience-reconnect/examples/reconnect_custom_policy.rs)

### Health Check

Proactive health monitoring with intelligent resource selection:

```rust
use tower_resilience_healthcheck::{HealthCheckWrapper, HealthStatus, SelectionStrategy};
use std::time::Duration;

// Create wrapper with multiple resources
let wrapper = HealthCheckWrapper::builder()
    .with_context(primary_db, "primary")
    .with_context(secondary_db, "secondary")
    .with_checker(|db| async move {
        match db.ping().await {
            Ok(_) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        }
    })
    .with_interval(Duration::from_secs(5))
    .with_selection_strategy(SelectionStrategy::RoundRobin)
    .build();

// Start background health checking
wrapper.start().await;

// Get a healthy resource
if let Some(db) = wrapper.get_healthy().await {
    // Use healthy database
}
```

**Note:** Health Check is not a Tower layer - it's a wrapper pattern for managing multiple resources with automatic failover.

**Full examples:** [healthcheck_basic.rs](crates/tower-resilience-healthcheck/examples/healthcheck_basic.rs)

### Coalesce

Deduplicate concurrent identical requests (singleflight pattern):

```rust
use tower_resilience_coalesce::CoalesceLayer;
use tower::ServiceBuilder;

// Coalesce by request ID - concurrent requests for same ID share one execution
let layer = CoalesceLayer::new(|req: &Request| req.id.clone());

let service = ServiceBuilder::new()
    .layer(layer)
    .service(my_service);

// Use with cache to prevent stampede on cache miss
let service = ServiceBuilder::new()
    .layer(cache_layer)      // Check cache first
    .layer(coalesce_layer)   // Coalesce cache misses
    .service(backend);
```

Use cases:
- **Cache stampede prevention**: When cache expires, only one request refreshes it
- **Expensive computations**: Deduplicate identical report generation requests
- **Rate-limited APIs**: Reduce calls to external APIs by coalescing identical requests

**Note:** Response and error types must implement `Clone` to be shared with all waiters.

### Executor

Delegate request processing to dedicated executors for parallel execution:

```rust
use tower_resilience_executor::ExecutorLayer;
use tower::ServiceBuilder;

// Use a dedicated runtime for CPU-heavy work
let compute_runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(8)
    .thread_name("compute")
    .build()
    .unwrap();

let layer = ExecutorLayer::new(compute_runtime.handle().clone());

// Or use the current runtime
let layer = ExecutorLayer::current();

let service = ServiceBuilder::new()
    .layer(layer)
    .service(my_service);
```

Use cases:
- **CPU-bound processing**: Parallelize CPU-intensive request handling
- **Runtime isolation**: Process requests on a dedicated runtime
- **Thread pool delegation**: Use specific thread pools for certain workloads

### Adaptive Concurrency

Dynamically adjust concurrency limits based on observed latency and error rates:

```rust
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, Vegas};
use tower::ServiceBuilder;
use std::time::Duration;

// AIMD: Classic TCP-style congestion control
// Increases limit on success, decreases on failure/high latency
let layer = AdaptiveLimiterLayer::new(
    Aimd::builder()
        .initial_limit(10)
        .min_limit(1)
        .max_limit(100)
        .increase_by(1)                           // Add 1 on success
        .decrease_factor(0.5)                     // Halve on failure
        .latency_threshold(Duration::from_millis(100))
        .build()
);

// Vegas: More stable, uses RTT to estimate queue depth
let layer = AdaptiveLimiterLayer::new(
    Vegas::builder()
        .initial_limit(10)
        .alpha(3)    // Increase when queue < 3
        .beta(6)     // Decrease when queue > 6
        .build()
);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(my_service);
```

Use cases:
- **Auto-tuning**: No manual concurrency limit configuration needed
- **Variable backends**: Adapts to changing downstream capacity
- **Load shedding**: Automatically reduces load when backends struggle

**Full examples:** [adaptive.rs](examples/adaptive.rs)

### Chaos (Testing Only)

Inject failures and latency to test your resilience patterns:

```rust
use tower_resilience_chaos::ChaosLayer;
use std::time::Duration;

// Types inferred from closure signature - no type parameters needed!
let chaos = ChaosLayer::builder()
    .name("test-chaos")
    .error_rate(0.1)                               // 10% of requests fail
    .error_fn(|_req: &String| std::io::Error::new(
        std::io::ErrorKind::Other, "chaos!"
    ))
    .latency_rate(0.2)                             // 20% delayed
    .min_latency(Duration::from_millis(50))
    .max_latency(Duration::from_millis(200))
    .seed(42)                                      // Deterministic chaos
    .build();

let service = chaos.layer(my_service);
```

**WARNING**: Only use in development/testing environments. Never in production.

**Full examples:** [chaos.rs](examples/chaos.rs) | [chaos_example.rs](crates/tower-resilience-chaos/examples/chaos_example.rs)

## Error Handling

When composing multiple resilience layers, each layer has its own error type (e.g., `CircuitBreakerError`, `BulkheadError`). The `ResilienceError<E>` type unifies these into a single error type, eliminating boilerplate.

### The Problem

Without a unified error type, you'd need `From` implementations for every layer combination:

```rust
// Without ResilienceError: ~80 lines of boilerplate for 4 layers
impl From<BulkheadError> for ServiceError { /* ... */ }
impl From<CircuitBreakerError> for ServiceError { /* ... */ }
impl From<RateLimiterError> for ServiceError { /* ... */ }
impl From<TimeLimiterError> for ServiceError { /* ... */ }
```

### The Solution

Use `ResilienceError<E>` as your service error type - all layer errors automatically convert:

```rust
use tower_resilience_core::ResilienceError;

// Your application error
#[derive(Debug, Clone)]
enum AppError {
    DatabaseDown,
    InvalidRequest,
}

// That's it! Zero From implementations needed
type ServiceError = ResilienceError<AppError>;
```

### Pattern Matching

Handle different failure modes explicitly:

```rust
use tower_resilience_core::ResilienceError;

fn handle_error<E: std::fmt::Display>(error: ResilienceError<E>) {
    match error {
        ResilienceError::Timeout { layer } => {
            eprintln!("Timeout in {}", layer);
        }
        ResilienceError::CircuitOpen { name } => {
            eprintln!("Circuit breaker {:?} is open - fail fast", name);
        }
        ResilienceError::BulkheadFull { concurrent_calls, max_concurrent } => {
            eprintln!("Bulkhead full: {}/{} - try again later", concurrent_calls, max_concurrent);
        }
        ResilienceError::RateLimited { retry_after } => {
            if let Some(duration) = retry_after {
                eprintln!("Rate limited, retry after {:?}", duration);
            }
        }
        ResilienceError::Application(app_err) => {
            eprintln!("Application error: {}", app_err);
        }
    }
}
```

### Helper Methods

Quickly check error categories:

```rust
if err.is_timeout() {
    // Handle timeout from any layer (TimeLimiter or Bulkhead)
}

if err.is_circuit_open() {
    // Circuit breaker is protecting the system
}

if err.is_rate_limited() {
    // Backpressure - slow down
}

if err.is_application() {
    // Get the underlying application error
    if let Some(app_err) = err.application_error() {
        // Handle app-specific error
    }
}
```

### When to Use

**Use `ResilienceError<E>` when:**
- Building new services with multiple resilience layers
- You want zero boilerplate error handling
- Standard error categorization is sufficient

**Use manual `From` implementations when:**
- You need very specific error semantics
- Integrating with legacy error types
- You need specialized error logging per layer

See the [`tower_resilience_core::error`](https://docs.rs/tower-resilience-core/latest/tower_resilience_core/error/) module for full documentation.

## Pattern Composition

Stack multiple patterns for comprehensive resilience:

```rust
use tower::ServiceBuilder;

// Client-side: timeout -> circuit breaker -> retry
let client = ServiceBuilder::new()
    .layer(timeout_layer)
    .layer(circuit_breaker_layer)
    .layer(retry_layer)
    .service(http_client);

// Server-side: rate limit -> bulkhead -> timeout
let server = ServiceBuilder::new()
    .layer(rate_limiter_layer)
    .layer(bulkhead_layer)
    .layer(timeout_layer)
    .service(handler);
```

For comprehensive guidance on composing patterns effectively, see:

- **[Composition Guide](https://docs.rs/tower-resilience/latest/tower_resilience/composition/)** - Pattern selection, recommended stacks, layer ordering, and anti-patterns
- **[Composition Tests](tests/composition_stacks/)** - Working examples of all documented stacks that verify correct compilation

## Benchmarks

Happy path overhead (no failures triggered):

| Pattern | Overhead |
|---------|----------|
| Retry (no retries) | ~80-100 ns |
| Time Limiter | ~107 ns |
| Rate Limiter | ~124 ns |
| Bulkhead | ~162 ns |
| Cache (hit) | ~250 ns |
| Circuit Breaker (closed) | ~298 ns |

```bash
cargo bench --bench happy_path_overhead
```

## Examples

```bash
cargo run --example circuitbreaker
cargo run --example bulkhead
cargo run --example retry
```

See [examples/](examples/) for more.

## Stress Tests

```bash
cargo test --test stress -- --ignored
```

## MSRV

1.64.0 (matches Tower)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please see the [contributing guidelines](CONTRIBUTING.md) for more information.
