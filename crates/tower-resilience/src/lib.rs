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
//! let service = ServiceBuilder::new()
//!     .layer(
//!         CircuitBreakerConfig::builder()
//!             .failure_rate_threshold(0.5)
//!             .sliding_window_size(100)
//!             .build()
//!     )
//!     .layer(
//!         BulkheadConfig::builder()
//!             .max_concurrent_calls(10)
//!             .build()
//!     )
//!     .service(my_service);
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
//! - `tower-resilience-core` (shared infrastructure)

// Re-export core (always available)
pub use tower_resilience_core as core;

// Re-export patterns based on features
#[cfg(feature = "circuitbreaker")]
pub use tower_circuitbreaker as circuitbreaker;

#[cfg(feature = "bulkhead")]
pub use tower_bulkhead as bulkhead;
