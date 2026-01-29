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
//! tower-resilience = { version = "0.5", features = ["circuitbreaker", "bulkhead"] }
//! ```
//!
//! # Presets: Get Started Immediately
//!
//! Every pattern includes **preset configurations** with sensible defaults.
//! Start immediately without tuning parameters - customize later when needed:
//!
//! ```rust,no_run
//! # #[cfg(all(feature = "retry", feature = "circuitbreaker", feature = "ratelimiter", feature = "bulkhead"))]
//! # {
//! use tower_resilience::retry::RetryLayer;
//! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
//! use tower_resilience::ratelimiter::RateLimiterLayer;
//! use tower_resilience::bulkhead::BulkheadLayer;
//!
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! // Retry: 3 attempts with 100ms exponential backoff
//! let retry = RetryLayer::<(), MyError>::exponential_backoff().build();
//!
//! // Circuit breaker: balanced defaults (50% threshold, 100 call window)
//! let breaker = CircuitBreakerLayer::standard().build();
//!
//! // Rate limiter: 100 requests per second
//! let limiter = RateLimiterLayer::per_second(100).build();
//!
//! // Bulkhead: 50 concurrent calls
//! let bulkhead = BulkheadLayer::medium().build();
//! # }
//! ```
//!
//! ## Available Presets
//!
//! | Pattern | Presets |
//! |---------|---------|
//! | **Retry** | [`exponential_backoff()`], [`aggressive()`], [`conservative()`] |
//! | **Circuit Breaker** | [`standard()`], [`fast_fail()`], [`tolerant()`] |
//! | **Rate Limiter** | [`per_second(n)`], [`per_minute(n)`], [`burst(rate, size)`] |
//! | **Bulkhead** | [`small()`], [`medium()`], [`large()`] |
//!
//! Presets return builders, so you can customize any setting:
//!
//! ```rust,no_run
//! # #[cfg(feature = "circuitbreaker")]
//! # {
//! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
//! use std::time::Duration;
//!
//! let breaker = CircuitBreakerLayer::fast_fail()
//!     .name("payment-api")
//!     .wait_duration_in_open(Duration::from_secs(30))
//!     .build();
//! # }
//! ```
//!
//! [`exponential_backoff()`]: retry::RetryLayer::exponential_backoff
//! [`aggressive()`]: retry::RetryLayer::aggressive
//! [`conservative()`]: retry::RetryLayer::conservative
//! [`standard()`]: circuitbreaker::CircuitBreakerLayer::standard
//! [`fast_fail()`]: circuitbreaker::CircuitBreakerLayer::fast_fail
//! [`tolerant()`]: circuitbreaker::CircuitBreakerLayer::tolerant
//! [`per_second(n)`]: ratelimiter::RateLimiterLayer::per_second
//! [`per_minute(n)`]: ratelimiter::RateLimiterLayer::per_minute
//! [`burst(rate, size)`]: ratelimiter::RateLimiterLayer::burst
//! [`small()`]: bulkhead::BulkheadLayer::small
//! [`medium()`]: bulkhead::BulkheadLayer::medium
//! [`large()`]: bulkhead::BulkheadLayer::large
//!
//! # Resilience Patterns
//!
//! - **[Circuit Breaker]** - Prevents cascading failures by stopping calls to failing services
//! - **[Bulkhead]** - Isolates resources to prevent system-wide failures
//! - **[Time Limiter]** - Advanced timeout handling with cancellation support
//! - **[Retry]** - Intelligent retry with exponential backoff and jitter
//! - **[Rate Limiter]** - Controls request rate to protect services
//! - **[Cache]** - Response memoization to reduce load
//! - **[Reconnect]** - Automatic reconnection with configurable backoff strategies
//! - **[Health Check]** - Proactive health monitoring with intelligent resource selection
//! - **[Fallback]** - Provides alternative responses when services fail
//! - **[Hedge]** - Reduces tail latency by firing parallel requests
//! - **[Executor]** - Delegates request processing to dedicated executors
//! - **[Adaptive]** - Dynamic concurrency limiting using AIMD or Vegas algorithms
//! - **[Coalesce]** - Deduplicates concurrent identical requests (singleflight)
//!
//! [Circuit Breaker]: https://docs.rs/tower-resilience-circuitbreaker
//! [Bulkhead]: https://docs.rs/tower-resilience-bulkhead
//! [Time Limiter]: https://docs.rs/tower-resilience-timelimiter
//! [Retry]: https://docs.rs/tower-resilience-retry
//! [Rate Limiter]: https://docs.rs/tower-resilience-ratelimiter
//! [Cache]: https://docs.rs/tower-resilience-cache
//! [Reconnect]: https://docs.rs/tower-resilience-reconnect
//! [Health Check]: https://docs.rs/tower-resilience-healthcheck
//! [Fallback]: https://docs.rs/tower-resilience-fallback
//! [Hedge]: https://docs.rs/tower-resilience-hedge
//! [Executor]: https://docs.rs/tower-resilience-executor
//! [Adaptive]: https://docs.rs/tower-resilience-adaptive
//! [Coalesce]: https://docs.rs/tower-resilience-coalesce
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
//! let circuit_breaker = CircuitBreakerLayer::builder()
//!     .name("api-client")
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(100)
//!     .build();
//!
//! let retry = RetryLayer::<(), MyError>::builder()
//!     .name("api-retry")
//!     .max_attempts(3)
//!     .exponential_backoff(Duration::from_millis(100))
//!     .build();
//!
//! // Compose manually for reliability
//! let resilient_client = retry.layer(http_client);
//! let resilient_client = circuit_breaker.layer(resilient_client);
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
//!
//! # Error Handling
//!
//! When composing multiple resilience layers, each layer has its own error type.
//! [`core::ResilienceError<E>`] unifies these into a single error type, eliminating
//! boilerplate `From` implementations.
//!
//! ## Quick Setup
//!
//! ```rust
//! use tower_resilience_core::ResilienceError;
//!
//! // Your application error
//! #[derive(Debug, Clone)]
//! enum AppError {
//!     DatabaseDown,
//!     InvalidRequest,
//! }
//!
//! impl std::fmt::Display for AppError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         match self {
//!             AppError::DatabaseDown => write!(f, "Database down"),
//!             AppError::InvalidRequest => write!(f, "Invalid request"),
//!         }
//!     }
//! }
//!
//! impl std::error::Error for AppError {}
//!
//! // Use ResilienceError as your service error type - zero From impls needed!
//! type ServiceError = ResilienceError<AppError>;
//! ```
//!
//! ## Pattern Matching
//!
//! ```rust
//! use tower_resilience_core::ResilienceError;
//!
//! # #[derive(Debug)]
//! # struct AppError;
//! # impl std::fmt::Display for AppError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
//! # }
//! # impl std::error::Error for AppError {}
//! fn handle_error(error: ResilienceError<AppError>) {
//!     match error {
//!         ResilienceError::Timeout { layer } => {
//!             eprintln!("Timeout in {}", layer);
//!         }
//!         ResilienceError::CircuitOpen { name } => {
//!             eprintln!("Circuit breaker {:?} is open", name);
//!         }
//!         ResilienceError::BulkheadFull { concurrent_calls, max_concurrent } => {
//!             eprintln!("Bulkhead full: {}/{}", concurrent_calls, max_concurrent);
//!         }
//!         ResilienceError::RateLimited { retry_after } => {
//!             eprintln!("Rate limited, retry after {:?}", retry_after);
//!         }
//!         ResilienceError::Application(app_err) => {
//!             eprintln!("Application error: {}", app_err);
//!         }
//!     }
//! }
//! ```
//!
//! ## Helper Methods
//!
//! ```rust
//! # use tower_resilience_core::ResilienceError;
//! # #[derive(Debug)]
//! # struct AppError;
//! # impl std::fmt::Display for AppError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
//! # }
//! # impl std::error::Error for AppError {}
//! # fn example(error: ResilienceError<AppError>) {
//! // Quickly check error categories
//! if error.is_timeout() {
//!     // Handle timeout from TimeLimiter or Bulkhead
//! } else if error.is_circuit_open() {
//!     // Circuit breaker protecting the system
//! } else if error.is_rate_limited() {
//!     // Backpressure - slow down
//! } else if error.is_application() {
//!     // Extract the underlying application error
//!     let app_err = error.application_error();
//! }
//! # }
//! ```
//!
//! For complete documentation, see [`core::ResilienceError`].

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

#[cfg(feature = "reconnect")]
pub use tower_resilience_reconnect as reconnect;

#[cfg(feature = "healthcheck")]
pub use tower_resilience_healthcheck as healthcheck;

#[cfg(feature = "fallback")]
pub use tower_resilience_fallback as fallback;

#[cfg(feature = "hedge")]
pub use tower_resilience_hedge as hedge;

#[cfg(feature = "executor")]
pub use tower_resilience_executor as executor;

#[cfg(feature = "adaptive")]
pub use tower_resilience_adaptive as adaptive;

#[cfg(feature = "coalesce")]
pub use tower_resilience_coalesce as coalesce;
