# tower-resilience

A comprehensive resilience and fault-tolerance toolkit for Tower services, inspired by Resilience4j.

## Overview

Tower-resilience provides a suite of resilience patterns for building fault-tolerant distributed systems:

- **Circuit Breaker** - Prevents cascading failures by stopping calls to failing services
- **Bulkhead** - Isolates resources to prevent system-wide failures
- **Time Limiter** - Advanced timeout handling with cancellation support
- **Retry** - Intelligent retry with exponential backoff and jitter
- **Rate Limiter** - Controls request rate to protect services
- **Cache** - Response memoization to reduce load

All components are built as composable Tower middleware with unified event system, comprehensive metrics, and shared configuration patterns.

## Quick Start

```toml
[dependencies]
tower-resilience = "0.1"
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

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
