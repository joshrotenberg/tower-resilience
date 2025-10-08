//! Core infrastructure for tower-resilience.
//!
//! This crate provides shared functionality used across all tower-resilience modules:
//! - Event system for observability
//! - Metrics infrastructure
//! - Common configuration patterns
//! - Registry for managing instances

pub mod events;

pub use events::{EventListener, ResilienceEvent};
