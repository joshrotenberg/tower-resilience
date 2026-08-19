//! Automatic reconnection middleware for Tower services.
//!
//! This crate provides reconnection functionality for Tower services with configurable
//! backoff strategies, connection state management, and comprehensive event system.
//!
//! # Features
//!
//! - **Automatic reconnection**: Detect connection failures and reconnect automatically
//! - **Flexible backoff**: Reuse `IntervalFunction` from retry module (exponential, linear, fixed)
//! - **Connection state tracking**: Monitor connection health and reconnection attempts
//! - **Event system**: Observability through reconnection events
//! - **Idempotency control**: Optional retry of original request after reconnection
//!
//! # Reconnect vs Retry: When to Use Each
//!
//! ## Use Reconnect When:
//!
//! - Managing **persistent connections** (TCP, WebSocket, database connections, Redis)
//! - You need **connection state tracking** (Connected/Disconnected/Reconnecting)
//! - Errors indicate the **connection itself is broken** (not just a failed request)
//! - You want to **reconnect automatically** but control whether to retry the operation
//!
//! ## Use Retry When:
//!
//! - Handling **transient operation failures** on a working connection
//! - Errors are **request-level** (rate limiting, temporary server errors, timeouts)
//! - You want to **retry the same operation** without reconnecting
//! - Connection is fine, just the specific request failed
//!
//! ## Composing Both (Persistent Connections):
//!
//! For services with persistent connections (Redis, databases, gRPC), you often want BOTH:
//!
//! ```rust,no_run
//! # use tower::ServiceBuilder;
//! # use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
//! # use std::time::Duration;
//! # async fn example() {
//! # let make_connection = tower::service_fn(|(): ()| async {
//! #     Ok::<_, std::io::Error>(tower::service_fn(|_req: ()| async {
//! #         Ok::<_, std::io::Error>(())
//! #     }))
//! # });
//! let service = ServiceBuilder::new()
//!     // Outer: Retry transient request errors (tower_resilience_retry::RetryLayer)
//!     // Inner: Reconnect on connection failures
//!     .layer(ReconnectLayer::new(
//!         ReconnectConfig::builder()
//!             .policy(ReconnectPolicy::exponential(
//!                 Duration::from_millis(100),
//!                 Duration::from_secs(5),
//!             ))
//!             .retry_on_reconnect(false)  // Let outer retry layer handle it
//!             .build()
//!     ))
//!     .service(make_connection);
//! # }
//! ```
//!
//! This provides:
//! - **Connection resilience** via reconnect layer (handles broken pipes, connection resets)
//! - **Operation resilience** via retry layer (handles rate limits, temporary errors)
//! - **Clear separation** of concerns
//! - **Fine-grained control** over idempotency
//!
//! # Examples
//!
//! ## Basic Reconnect with Exponential Backoff
//!
//! ```rust
//! use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
//! use tower::{service_fn, Layer};
//! use std::convert::Infallible;
//! use std::time::Duration;
//!
//! // Create reconnect configuration
//! let config = ReconnectConfig::builder()
//!     .policy(ReconnectPolicy::exponential(
//!         Duration::from_millis(100),
//!         Duration::from_secs(5),
//!     ))
//!     .max_attempts(10)
//!     .retry_on_reconnect(true)  // Safe for idempotent operations
//!     .build();
//!
//! // The layer wraps a factory. A reconnect invokes it again and receives a
//! // distinct service instance.
//! let factory = service_fn(|(): ()| async {
//!     Ok::<_, Infallible>(service_fn(|request: String| async move {
//!         Ok::<_, std::io::Error>(request)
//!     }))
//! });
//! let reconnect_service = ReconnectLayer::new(config).layer(factory);
//! ```
//!
//! ## Non-Idempotent Operations
//!
//! For operations that should not be automatically retried (e.g., Redis INCR, LPUSH):
//!
//! ```rust
//! use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
//! use std::time::Duration;
//!
//! let config = ReconnectConfig::builder()
//!     .policy(ReconnectPolicy::exponential(
//!         Duration::from_millis(100),
//!         Duration::from_secs(5),
//!     ))
//!     .retry_on_reconnect(false)  // Reconnect but DON'T retry the operation
//!     .build();
//!
//! let layer = ReconnectLayer::new(config);
//!
//! // User handles the error and decides whether retry is safe:
//! // - Was the operation executed before the connection died?
//! // - Can we safely retry without duplicating side effects?
//! ```

mod config;
mod layer;
mod policy;
mod service;
mod state;

/// Migration guide for the post-0.11 factory-backed reconnect API.
#[doc = include_str!("../MIGRATION.md")]
pub mod migration {}

pub use config::{ReconnectConfig, ReconnectConfigBuilder, ReconnectPredicate};
pub use layer::ReconnectLayer;
pub use policy::ReconnectPolicy;
pub use service::{ReconnectError, ReconnectService};
pub use state::{ConnectionState, ReconnectState};

// Re-export backoff strategies from retry crate for convenience
pub use tower_resilience_retry::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, IntervalFunction,
};
