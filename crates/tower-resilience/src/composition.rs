//! # Composition Guide
//!
//! Comprehensive guide to composing resilience patterns together, including layer ordering,
//! error type integration, and workarounds for complex compositions.

/// Common composition patterns
pub mod patterns {
    //! # Composition Patterns
    //!
    //! Patterns are designed to be composed together for comprehensive resilience.
    //!
    //! ## Inbound (Server-Side)
    //!
    //! Protect your service from abusive or overwhelming clients:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │  Rate Limiter   │ ← Reject abusive clients
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │    Bulkhead     │ ← Isolate tenant resources
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Prevent runaway requests
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │     Handler     │
    //! └─────────────────┘
    //! ```
    //!
    //! ## Outbound (Client-Side)
    //!
    //! Make your clients resilient to downstream failures:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Don't wait forever
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │ Circuit Breaker │ ← Fail fast when down
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │      Retry      │ ← Handle transient errors
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │     Client      │
    //! └─────────────────┘
    //! ```
    //!
    //! ## Read-Through Cache
    //!
    //! Cache expensive operations with resilience:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │      Cache      │ ← Try cache first
    //! └────────┬────────┘
    //!          │ (miss)
    //!          ▼
    //! ┌─────────────────┐
    //! │ Circuit Breaker │ ← Protect backend
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Bound latency
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │    Backend      │
    //! └─────────────────┘
    //! ```
}

/// Layer ordering guide
pub mod ordering {
    //! # Layer Ordering
    //!
    //! Layer order is critical! Layers execute **outside-in** (first layer in builder executes last).
    //!
    //! ## Client-Side (Outbound)
    //!
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(cache)              // 1st: Check cache before anything
    //!     .layer(timeout)            // 2nd: Enforce overall deadline
    //!     .layer(circuit_breaker)    // 3rd: Fail fast if down
    //!     .layer(retry)              // 4th: Retry transient failures (innermost, closest to service)
    //!     .service(http_client);
    //! ```
    //!
    //! **Rationale**:
    //! - **Cache** outermost: Skip all other layers on cache hit
    //! - **Timeout** next: Enforce deadline across retries and circuit breaker
    //! - **Circuit breaker** before retry: Don't retry when circuit is open
    //! - **Retry** innermost: Retry individual failures before circuit breaker sees them
    //!
    //! ## Server-Side (Inbound)
    //!
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(rate_limiter)       // 1st: Reject abusive clients immediately
    //!     .layer(bulkhead)           // 2nd: Isolate resources per tenant
    //!     .layer(timeout)            // 3rd: Prevent runaway requests (innermost)
    //!     .service(handler);
    //! ```
    //!
    //! **Rationale**:
    //! - **Rate limiter** outermost: Reject over-limit requests before consuming resources
    //! - **Bulkhead** next: Isolate resources after rate limiting
    //! - **Timeout** innermost: Apply to actual handler execution
}

/// Error type integration strategies
pub mod error_types {
    //! # Error Type Integration
    //!
    //! When composing multiple resilience layers, all layers must agree on error types.
    //! Tower-resilience provides three approaches, from simplest to most flexible.
    //!
    //! ## 1. `ResilienceError<E>` (Recommended - Zero Boilerplate)
    //!
    //! Use the provided [`ResilienceError<E>`](tower_resilience_core::ResilienceError) type
    //! to eliminate manual `From` implementations:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "bulkhead", feature = "ratelimiter"))]
    //! # {
    //! use tower::ServiceBuilder;
    //! use tower_resilience_core::ResilienceError;
    //! use tower_resilience_bulkhead::BulkheadLayer;
    //! use tower_resilience::ratelimiter::RateLimiterLayer;
    //! use std::time::Duration;
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
    //!             AppError::InvalidRequest => write!(f, "Invalid"),
    //!         }
    //!     }
    //! }
    //!
    //! impl std::error::Error for AppError {}
    //!
    //! // That's it! Zero From implementations needed
    //! type ServiceError = ResilienceError<AppError>;
    //!
    //! // All resilience layer errors automatically convert to ResilienceError
    //! // let service = ServiceBuilder::new()
    //! //     .layer(bulkhead)
    //! //     .layer(rate_limiter)
    //! //     .service(my_service);
    //! # }
    //! ```
    //!
    //! **Benefits:**
    //! - Zero boilerplate - no manual `From` implementations
    //! - Works with any number of layers
    //! - Rich error context (layer names, counts, durations)
    //! - Convenient helpers: `is_timeout()`, `is_rate_limited()`, etc.
    //! - Application errors wrapped in `Application` variant
    //!
    //! **Use when:**
    //! - Building new services
    //! - You want to move fast with minimal code
    //! - Standard error categorization is sufficient
    //!
    //! ## 2. Custom Error Type with Manual From
    //!
    //! Define your own error type and implement `From` for each layer:
    //!
    //! ```rust,no_run
    //! # use std::time::Duration;
    //! # #[cfg(all(feature = "retry", feature = "circuitbreaker"))]
    //! # {
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //!
    //! #[derive(Debug, Clone)]
    //! enum ServiceError {
    //!     Network(String),
    //!     Timeout,
    //!     CircuitOpen,
    //!     RateLimit,
    //! }
    //!
    //! // Manual From implementations give you full control
    //! // impl From<BulkheadError> for ServiceError { /* ... */ }
    //! // impl From<CircuitBreakerError> for ServiceError { /* ... */ }
    //!
    //! let retry = RetryLayer::<ServiceError>::builder()
    //!     .max_attempts(3)
    //!     .retry_on(|err| matches!(err, ServiceError::Network(_)))
    //!     .build();
    //! # }
    //! ```
    //!
    //! **Use when:**
    //! - You need very specific error semantics
    //! - Different recovery strategies per layer
    //! - Integrating with legacy error types
    //! - Custom error logging requirements
    //!
    //! ## 3. Error Mapping Layer
    //!
    //! Use `tower::util::MapErr` to convert between error types:
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "retry")]
    //! # {
    //! use tower::{ServiceBuilder, ServiceExt};
    //! use tower_resilience::retry::RetryLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug)]
    //! # struct DatabaseError;
    //! # #[derive(Debug, Clone)]
    //! # struct AppError;
    //! # impl From<DatabaseError> for AppError {
    //! #     fn from(_: DatabaseError) -> Self { AppError }
    //! # }
    //! # async fn example() {
    //! # let db_service = tower::service_fn(|_req: ()| async { Ok::<_, DatabaseError>(()) });
    //! let service = ServiceBuilder::new()
    //!     .layer(RetryLayer::<AppError>::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .map_err(|err: DatabaseError| AppError::from(err))
    //!     .service(db_service);
    //! # }
    //! # }
    //! ```
}

/// Advanced composition techniques
pub mod advanced {
    //! # Advanced Composition
    //!
    //! ## Overview
    //!
    //! Tower-resilience patterns are designed to compose together using Tower's `ServiceBuilder`.
    //! However, composing 3+ layers can encounter Rust trait bound limitations. This guide
    //! explains successful patterns and workarounds.
    //!
    //! ## Basic Composition (2 Layers)
    //!
    //! Two-layer composition works reliably with `ServiceBuilder`:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter"))]
    //! # {
    //! use tower::ServiceBuilder;
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let service = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
    //! let composed = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .layer(RetryLayer::<MyError>::builder()
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .service(service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Limitations with 3+ Layers
    //!
    //! **Problem**: Composing 3+ resilience layers using `ServiceBuilder` often hits Rust
    //! trait bound limitations. This is a known issue with complex Tower layer stacks.
    //!
    //! **Why it happens**:
    //! - Each layer wraps the service in a new type
    //! - Trait bounds become deeply nested
    //! - Rust's type inference struggles with complex layer stacks
    //! - Some combinations trigger "overflow evaluating the requirement" errors
    //!
    //! **Example that may fail**:
    //!
    //! ```rust,ignore
    //! // This may encounter trait bound errors
    //! ServiceBuilder::new()
    //!     .layer(cache_layer)
    //!     .layer(circuit_breaker)
    //!     .layer(retry_layer)
    //!     .layer(timeout_layer)
    //!     .service(base_service);  // Error: trait bounds not satisfied
    //! ```
    //!
    //! ## Workarounds
    //!
    //! ### 1. Manual Layer Composition
    //!
    //! Apply layers one at a time manually (most reliable):
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "circuitbreaker", feature = "cache"))]
    //! # {
    //! use tower::Layer;
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //! use tower_resilience_cache::CacheLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # #[derive(Clone)]
    //! # struct Request { id: u64 }
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|req: Request| async { Ok::<_, MyError>(req) });
    //! // Build layers inside-out manually
    //! let with_retry = RetryLayer::<MyError>::builder()
    //!     .max_attempts(3)
    //!     .build()
    //!     .layer(base_service);
    //!
    //! let with_circuit_breaker = CircuitBreakerLayer::<Request, MyError>::builder()
    //!     .failure_rate_threshold(0.5)
    //!     .build()
    //!     .layer(with_retry);
    //!
    //! let service = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id)
    //!     .build()
    //!     .layer(with_circuit_breaker);
    //! # }
    //! # }
    //! ```
    //!
    //! ### 2. Limit ServiceBuilder Stack Depth
    //!
    //! Keep ServiceBuilder stacks to 2-3 layers max, compose manually beyond that:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter", feature = "cache"))]
    //! # {
    //! use tower::{ServiceBuilder, Layer};
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use tower_resilience_cache::CacheLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # #[derive(Clone)]
    //! # struct Request { id: u64 }
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|req: Request| async { Ok::<_, MyError>(req) });
    //! // First 2 layers via ServiceBuilder
    //! let inner = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .layer(RetryLayer::<MyError>::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .service(base_service);
    //!
    //! // Additional layers manually
    //! let service = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id)
    //!     .build()
    //!     .layer(inner);
    //! # }
    //! # }
    //! ```
    //!
    //! ### 3. Split Complex Compositions
    //!
    //! For very complex stacks, split into logical groups and compose separately:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter"))]
    //! # {
    //! use tower::{ServiceBuilder, Layer};
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
    //! // Build retry layer first
    //! let retry_layer = RetryLayer::<MyError>::builder()
    //!     .max_attempts(3)
    //!     .build();
    //!
    //! // Apply retry manually
    //! let with_retry = retry_layer.layer(base_service);
    //!
    //! // Then use ServiceBuilder for remaining layers
    //! let service = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .service(with_retry);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Working Examples
    //!
    //! See the repository examples for complete, working compositions:
    //!
    //! - [`examples/composition_outbound.rs`] - Client-side resilience stack
    //! - [`examples/server_api.rs`] - Server-side protection
    //! - [`examples/database_client.rs`] - Database client with retry + circuit breaker
    //! - [`examples/message_queue_worker.rs`] - Message processing with bulkhead + retry
    //!
    //! [`examples/composition_outbound.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/composition_outbound.rs
    //! [`examples/server_api.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/server_api.rs
    //! [`examples/database_client.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/database_client.rs
    //! [`examples/message_queue_worker.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/message_queue_worker.rs
}
