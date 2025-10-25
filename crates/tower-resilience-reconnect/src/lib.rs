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
//! - **MakeService integration**: Works with any service that implements MakeService
//!
//! # Examples
//!
//! ## Basic Reconnect with Exponential Backoff
//!
//! ```rust
//! use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
//! use std::time::Duration;
//!
//! // Create reconnect configuration
//! let config = ReconnectConfig::builder()
//!     .policy(ReconnectPolicy::exponential(
//!         Duration::from_millis(100),
//!         Duration::from_secs(5),
//!     ))
//!     .max_attempts(10)
//!     .build();
//!
//! // Create the layer
//! let reconnect_layer = ReconnectLayer::new(config);
//! ```

mod config;
mod layer;
mod policy;
mod service;
mod state;

pub use config::{ReconnectConfig, ReconnectConfigBuilder};
pub use layer::ReconnectLayer;
pub use policy::ReconnectPolicy;
pub use service::ReconnectService;
pub use state::{ConnectionState, ReconnectState};

// Re-export backoff strategies from retry crate for convenience
pub use tower_resilience_retry::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, IntervalFunction,
};
