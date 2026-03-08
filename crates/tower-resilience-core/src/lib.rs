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

/// AIMD (Additive Increase / Multiplicative Decrease) controller.
pub mod aimd;
/// Failure classification traits and default implementations.
pub mod classifier;
/// Common error types for resilience patterns.
pub mod error;
/// Event system for resilience pattern observability.
pub mod events;

/// Unified error layer for composing resilience middleware.
#[cfg(feature = "layer")]
pub mod error_layer;

/// Health integration traits for proactive resilience.
#[cfg(feature = "health-integration")]
pub mod health_integration;

pub use aimd::{AimdConfig, AimdController};
pub use classifier::{DefaultClassifier, FailureClassifier, FnClassifier};
pub use error::{IntoResilienceError, ResilienceError};

#[cfg(feature = "layer")]
pub use error_layer::{ResilienceErrorLayer, ResilienceErrorService, UnifiedErrors};
pub use events::{EventListener, EventListeners, FnListener, ResilienceEvent};

#[cfg(feature = "health-integration")]
pub use health_integration::{HealthTriggerable, SharedHealthTrigger, TriggerHealth};
