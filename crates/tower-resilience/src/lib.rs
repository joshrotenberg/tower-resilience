//! Composable resilience and fault-tolerance middleware for Tower services.
//!
//! `tower-resilience` provides a collection of resilience patterns inspired by
//! [Resilience4j](https://resilience4j.readme.io/). Each pattern is available as both an
//! individual crate and as a feature in this meta-crate.
//!
//! # Quick Start
//!
//! ```toml
//! [dependencies]
//! tower-resilience = { version = "0.1", features = ["circuitbreaker", "bulkhead"] }
//! ```
//!
//! # Resilience Patterns
//!
//! - **[Circuit Breaker]** - Prevents cascading failures by stopping calls to failing services
//! - **[Bulkhead]** - Isolates resources to prevent system-wide failures
//! - **[Time Limiter]** - Advanced timeout handling with cancellation support
//! - **[Retry]** - Intelligent retry with exponential backoff and jitter
//! - **[Rate Limiter]** - Controls request rate to protect services
//! - **[Cache]** - Response memoization to reduce load
//!
//! [Circuit Breaker]: https://docs.rs/tower-resilience-circuitbreaker
//! [Bulkhead]: https://docs.rs/tower-resilience-bulkhead
//! [Time Limiter]: https://docs.rs/tower-resilience-timelimiter
//! [Retry]: https://docs.rs/tower-resilience-retry
//! [Rate Limiter]: https://docs.rs/tower-resilience-ratelimiter
//! [Cache]: https://docs.rs/tower-resilience-cache
//!
//! # Documentation Guides
//!
//! ## Getting Started
//!
//! - **[Tower Primer](tower_primer)** - Introduction to Tower concepts (Service, Layer, composition)
//! - **[Pattern Guides](patterns)** - Detailed guides for each pattern with examples and anti-patterns
//! - **[Composition Guide](composition)** - How to combine patterns effectively
//! - **[Use Cases](use_cases)** - Real-world scenarios and recommendations
//!
//! ## Observability
//!
//! - **[Metrics](observability::metrics)** - Prometheus metrics for all patterns
//! - **[Tracing](observability::tracing_guide)** - Structured logging setup
//! - **[Events](observability::events)** - Custom event listeners
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(all(feature = "circuitbreaker", feature = "retry"))]
//! # {
//! use tower::{ServiceBuilder, Layer};
//! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
//! use tower_resilience::retry::RetryLayer;
//! use std::time::Duration;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # impl std::fmt::Display for MyError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//! #         write!(f, "error")
//! #     }
//! # }
//! # impl std::error::Error for MyError {}
//! # async fn example() {
//! # let http_client = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
//! // Build a resilient HTTP client
//! let circuit_breaker = CircuitBreakerLayer::<(), MyError>::builder()
//!     .name("api-client")
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .build();
//!
//! let retry = RetryLayer::<MyError>::builder()
//!     .name("api-retry")
//!     .max_attempts(3)
//!     .exponential_backoff(Duration::from_millis(100))
//!     .build();
//!
//! // Compose manually for reliability
//! let resilient_client = retry.layer(http_client);
//! let resilient_client = circuit_breaker.layer::<_, ()>(resilient_client);
//! # }
//! # }
//! ```
//!
//! # Performance
//!
//! All patterns have low overhead in the happy path:
//!
//! - Retry: ~80-100ns (lightest)
//! - Time Limiter: ~107ns
//! - Rate Limiter: ~124ns
//! - Bulkhead: ~162ns
//! - Cache (hit): ~250ns
//! - Circuit Breaker: ~298ns (heaviest)
//!
//! See [benchmarks] for detailed measurements.
//!
//! [benchmarks]: https://github.com/joshrotenberg/tower-resilience#performance

// Documentation modules
pub mod composition;
pub mod observability;
pub mod patterns;
pub mod tower_primer;
pub mod use_cases;

// Re-export core (always available)
pub use tower_resilience_core as core;

// Re-export patterns based on features
#[cfg(feature = "circuitbreaker")]
pub use tower_resilience_circuitbreaker as circuitbreaker;

#[cfg(feature = "bulkhead")]
pub use tower_resilience_bulkhead as bulkhead;

#[cfg(feature = "timelimiter")]
pub use tower_resilience_timelimiter as timelimiter;

#[cfg(feature = "cache")]
pub use tower_resilience_cache as cache;

#[cfg(feature = "retry")]
pub use tower_resilience_retry as retry;

#[cfg(feature = "ratelimiter")]
pub use tower_resilience_ratelimiter as ratelimiter;
