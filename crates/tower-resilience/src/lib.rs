//! Composable resilience and fault-tolerance middleware for Tower services.
//!
//! `tower-resilience` provides a collection of resilience patterns that can be composed
//! together to build robust distributed systems. Each pattern is available as both an
//! individual crate and as a feature in this meta-crate.
//!
//! # Patterns
//!
//! - **Circuit Breaker** (`circuitbreaker` feature): Prevents cascading failures by
//!   temporarily blocking calls to failing services
//! - **Bulkhead** (`bulkhead` feature): Isolates resources by limiting concurrent calls
//! - **Time Limiter** (`timelimiter` feature): Advanced timeout handling with event system
//! - **Cache** (`cache` feature): Response memoization with LRU eviction and TTL
//! - **Retry** (`retry` feature): Enhanced retry with flexible backoff strategies
//!
//! # Usage
//!
//! Enable specific patterns via features:
//!
//! ```toml
//! [dependencies]
//! tower-resilience = { version = "0.1", features = ["circuitbreaker", "bulkhead"] }
//! ```
//!
//! Or enable all patterns:
//!
//! ```toml
//! [dependencies]
//! tower-resilience = { version = "0.1", features = ["full"] }
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(all(feature = "circuitbreaker", feature = "bulkhead"))]
//! # {
//! use tower::ServiceBuilder;
//! use tower_resilience::{circuitbreaker::CircuitBreakerConfig, bulkhead::BulkheadConfig};
//!
//! # async fn example() {
//! # let my_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
//! // Build bulkhead layer (implements Tower Layer trait)
//! let bulkhead_layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(10)
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(bulkhead_layer)
//!     .service(my_service);
//!
//! // Wrap with circuit breaker (uses manual .layer() method)
//! let cb_layer = CircuitBreakerConfig::<(), std::io::Error>::builder()
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .build();
//!
//! let _service = cb_layer.layer::<_, ()>(service);
//! # }
//! # }
//! ```
//!
//! # Individual Crates
//!
//! Each pattern is also available as a standalone crate for minimal dependencies:
//!
//! - `tower-circuitbreaker`
//! - `tower-bulkhead`
//! - `tower-timelimiter`
//! - `tower-cache`
//! - `tower-retry-plus`
//! - `tower-resilience-core` (shared infrastructure)

// Re-export core (always available)
pub use tower_resilience_core as core;

// Re-export patterns based on features
#[cfg(feature = "circuitbreaker")]
pub use tower_circuitbreaker as circuitbreaker;

#[cfg(feature = "bulkhead")]
pub use tower_bulkhead as bulkhead;

#[cfg(feature = "timelimiter")]
pub use tower_timelimiter as timelimiter;

#[cfg(feature = "cache")]
pub use tower_cache as cache;

#[cfg(feature = "retry")]
pub use tower_retry_plus as retry;
