# tower-resilience

[![Crates.io](https://img.shields.io/crates/v/tower-resilience.svg)](https://crates.io/crates/tower-resilience)
[![Documentation](https://docs.rs/tower-resilience/badge.svg)](https://docs.rs/tower-resilience)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.64.0%2B-blue.svg)](https://www.rust-lang.org)

A comprehensive resilience and fault-tolerance toolkit for [Tower](https://github.com/tower-rs/tower) services, inspired by [Resilience4j](https://resilience4j.readme.io/).

## About

Tower-resilience provides composable middleware for building robust distributed systems in Rust. [Tower](https://docs.rs/tower) is a library of modular and reusable components for building robust networking clients and servers. This crate extends Tower with resilience patterns commonly needed in production systems.

Inspired by [Resilience4j](https://resilience4j.readme.io/), a fault tolerance library for Java, tower-resilience adapts these battle-tested patterns to Rust's async ecosystem and Tower's middleware model.

## Resilience Patterns

- **Circuit Breaker** - Prevents cascading failures by stopping calls to failing services
- **Bulkhead** - Isolates resources to prevent system-wide failures  
- **Time Limiter** - Advanced timeout handling with cancellation support
- **Retry** - Intelligent retry with exponential backoff and jitter
- **Rate Limiter** - Controls request rate to protect services
- **Cache** - Response memoization to reduce load
- **Chaos** - Inject failures and latency for testing resilience (development/testing only)

## Features

- **Composable** - Stack multiple resilience patterns using Tower's ServiceBuilder
- **Observable** - Event system for monitoring pattern behavior (retries, state changes, etc.)
- **Configurable** - Builder APIs with sensible defaults
- **Async-first** - Built on tokio for async Rust applications
- **Zero-cost abstractions** - Minimal overhead when patterns aren't triggered

## Quick Start

```toml
[dependencies]
tower-resilience = "0.1"
tower = "0.5"
```

```rust
use tower::ServiceBuilder;
use tower_resilience::prelude::*;

let service = ServiceBuilder::new()
    .layer(CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .build())
    .layer(BulkheadLayer::builder()
        .max_concurrent_calls(10)
        .build())
    .service(my_service);
```

## Examples

### Circuit Breaker

Prevent cascading failures by opening the circuit when error rate exceeds threshold:

```rust
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use std::time::Duration;

let layer = CircuitBreakerLayer::<String, ()>::builder()
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
    .max_wait_duration(Some(Duration::from_secs(5)))  // Wait up to 5s
    .on_call_permitted(|concurrent| {
        println!("Request permitted (concurrent: {})", concurrent);
    })
    .on_call_rejected(|max| {
        println!("Request rejected (max: {})", max);
    })
    .build();

let service = layer.layer(my_service);
```

**Full examples:** [bulkhead.rs](examples/bulkhead.rs) | [bulkhead_demo.rs](crates/tower-resilience-bulkhead/examples/bulkhead_demo.rs)

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

let layer = RetryLayer::<MyError>::builder()
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

### Chaos (Testing Only)

Inject failures and latency to test your resilience patterns:

```rust
use tower_resilience_chaos::ChaosLayer;
use std::time::Duration;

let chaos = ChaosLayer::<String, std::io::Error>::builder()
    .name("test-chaos")
    .error_rate(0.1)                               // 10% of requests fail
    .error_fn(|_req| std::io::Error::new(
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

**Full examples:** [chaos_example.rs](crates/tower-resilience-chaos/examples/chaos_example.rs)

## Error Handling

### Zero-Boilerplate with ResilienceError

When composing multiple resilience layers, use `ResilienceError<E>` to eliminate manual error conversion code:

```rust
use tower_resilience_core::ResilienceError;

// Your application error
#[derive(Debug)]
enum AppError {
    DatabaseDown,
    InvalidRequest,
}

// That's it! No From implementations needed
type ServiceError = ResilienceError<AppError>;

// All resilience layer errors automatically convert
let service = ServiceBuilder::new()
    .layer(timeout_layer)
    .layer(circuit_breaker)
    .layer(bulkhead)
    .service(my_service);
```

**Benefits:**
- Zero boilerplate - no `From` trait implementations
- Rich error context (layer names, counts, durations)
- Convenient helpers: `is_timeout()`, `is_rate_limited()`, etc.

See the [Layer Composition Guide](https://docs.rs/tower-resilience) for details.

### Manual Error Handling

For specific use cases, you can still implement custom error types with manual `From` conversions. See examples for both approaches.

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

## Performance

Benchmarks measure the overhead of each pattern in the happy path (no failures, circuit closed, permits available):

| Pattern | Overhead (ns) | vs Baseline |
|---------|--------------|-------------|
| Baseline (no middleware) | ~10 ns | 1.0x |
| Retry (no retries) | ~80-100 ns | ~8-10x |
| Time Limiter | ~107 ns | ~10x |
| Rate Limiter | ~124 ns | ~12x |
| Bulkhead | ~162 ns | ~16x |
| Cache (hit) | ~250 ns | ~25x |
| Circuit Breaker (closed) | ~298 ns | ~29x |
| Circuit Breaker + Bulkhead | ~413 ns | ~40x |

**Key Takeaways:**
- All patterns add < 300ns overhead individually
- Overhead is additive when composing patterns
- Even the heaviest pattern (circuit breaker) is negligible for most use cases
- Retry and time limiter are the lightest weight options

Run benchmarks yourself:
```bash
cargo bench --bench happy_path_overhead
```

## Documentation

- [API Documentation](https://docs.rs/tower-resilience)
- [Pattern Guides](https://docs.rs/tower-resilience) - In-depth guides on when and how to use each pattern

### Examples

Two sets of examples are provided:

- **[Top-level examples](examples/)** - Simple, getting-started examples matching this README (one per pattern)
- **Module examples** - Detailed examples in each crate's `examples/` directory showing advanced features

Run top-level examples with:
```bash
cargo run --example circuitbreaker
cargo run --example bulkhead
cargo run --example retry
# etc.
```

## Stress Tests

Stress tests validate pattern behavior under extreme conditions (high volume, high concurrency, memory stability). They are opt-in and marked with `#[ignore]`:

```bash
# Run all stress tests
cargo test --test stress -- --ignored

# Run specific pattern stress tests
cargo test --test stress circuitbreaker -- --ignored
cargo test --test stress bulkhead -- --ignored
cargo test --test stress cache -- --ignored

# Run with output to see performance metrics
cargo test --test stress -- --ignored --nocapture
```

Example results:
- **1M calls** through circuit breaker: ~2.8s (357k calls/sec)
- **10k fast operations** through bulkhead: ~56ms (176k ops/sec)
- **100k cache** entries: Fill + hit test validates performance

Stress tests cover:
- High volume (millions of operations)
- High concurrency (thousands of concurrent requests)
- Memory stability (leak detection, bounded growth)
- State consistency (correctness under load)
- Pattern composition (layered middleware)

## Why tower-resilience?

Tower provides some built-in resilience (timeout, retry, rate limiting), but tower-resilience offers:

- **Circuit Breaker** - Not available in Tower
- **Advanced retry** - More backoff strategies and better control
- **Bulkhead** - True resource isolation with async-aware semaphores
- **Unified events** - Consistent observability across all patterns
- **Builder APIs** - Ergonomic configuration with sensible defaults
- **Production-ready** - Patterns inspired by battle-tested Resilience4j

## Minimum Supported Rust Version (MSRV)

This crate's MSRV is **1.64.0**, matching [Tower's MSRV policy](https://github.com/tower-rs/tower).

We follow Tower's approach:
- MSRV bumps are not considered breaking changes
- When increasing MSRV, the new version must have been released at least 6 months ago
- MSRV is tested in CI to prevent unintentional increases

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please see the [contributing guidelines](CONTRIBUTING.md) for more information.
