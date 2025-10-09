//! Core infrastructure for tower-resilience.
//!
//! This crate provides shared functionality used across all tower-resilience modules:
//! - Event system for observability
//! - Metrics infrastructure
//! - Common configuration patterns
//! - Registry for managing instances
//! - Common error types for resilience patterns

pub mod error;
pub mod events;

pub use error::ResilienceError;
pub use events::{EventListener, EventListeners, FnListener, ResilienceEvent};
