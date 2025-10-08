# tower-resilience

[![Crates.io](https://img.shields.io/crates/v/tower-resilience.svg)](https://crates.io/crates/tower-resilience)
[![Documentation](https://docs.rs/tower-resilience/badge.svg)](https://docs.rs/tower-resilience)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.85%2B-blue.svg)](https://www.rust-lang.org)

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

let layer = CircuitBreakerLayer::builder()
    .failure_rate_threshold(0.5)  // Open at 50% failure rate
    .sliding_window_size(100)      // Track last 100 calls
    .build();
```

See [examples/circuitbreaker.rs](examples/circuitbreaker.rs) for a complete example.

### Bulkhead

Limit concurrent requests to prevent resource exhaustion:

```rust
use tower_resilience_bulkhead::BulkheadLayer;

let layer = BulkheadLayer::builder()
    .max_concurrent_calls(10)
    .wait_timeout(Duration::from_secs(5))
    .build();
```

See [examples/bulkhead.rs](examples/bulkhead.rs) for a complete example.

### Time Limiter

Enforce timeouts on operations:

```rust
use tower_resilience_timelimiter::TimeLimiterConfig;

let config = TimeLimiterConfig::builder()
    .timeout_duration(Duration::from_secs(30))
    .cancel_running_future(true)
    .build();
```

### Retry

Retry failed requests with exponential backoff:

```rust
use tower_resilience_retry::{RetryConfig, ExponentialBackoff};

let config: RetryConfig<MyError> = RetryConfig::builder()
    .max_attempts(5)
    .exponential_backoff(Duration::from_millis(100))
    .build();
```

### Rate Limiter

Control request rate to protect downstream services:

```rust
use tower_resilience_ratelimiter::RateLimiterConfig;

let config = RateLimiterConfig::builder()
    .max_permits(100)
    .refresh_period(Duration::from_secs(1))
    .build();
```

### Cache

Cache responses to reduce load on expensive operations:

```rust
use tower_resilience_cache::CacheConfig;

let config = CacheConfig::builder()
    .max_capacity(1000)
    .ttl(Duration::from_secs(300))
    .key_extractor(|req: &Request| req.id.clone())
    .build();
```

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

## Why tower-resilience?

Tower provides some built-in resilience (timeout, retry, rate limiting), but tower-resilience offers:

- **Circuit Breaker** - Not available in Tower
- **Advanced retry** - More backoff strategies and better control
- **Bulkhead** - True resource isolation with async-aware semaphores
- **Unified events** - Consistent observability across all patterns
- **Builder APIs** - Ergonomic configuration with sensible defaults
- **Production-ready** - Patterns inspired by battle-tested Resilience4j

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please see the [contributing guidelines](CONTRIBUTING.md) for more information.
