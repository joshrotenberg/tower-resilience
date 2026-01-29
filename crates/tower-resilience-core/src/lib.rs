//! Core infrastructure for tower-resilience.
//!
//! This crate provides shared functionality used across all tower-resilience modules:
//! - Event system for observability
//! - Metrics infrastructure
//! - Common configuration patterns
//! - Registry for managing instances
//! - Common error types for resilience patterns
//! - AIMD controller for congestion control
//! - Health integration traits for proactive resilience

pub mod aimd;
pub mod error;
pub mod events;

#[cfg(feature = "health-integration")]
pub mod health_integration;

pub use aimd::{AimdConfig, AimdController};
pub use error::ResilienceError;
pub use events::{EventListener, EventListeners, FnListener, ResilienceEvent};

#[cfg(feature = "health-integration")]
pub use health_integration::{HealthTriggerable, SharedHealthTrigger, TriggerHealth};
