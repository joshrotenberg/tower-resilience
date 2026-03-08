//! Failure classification for circuit breaker decisions.
//!
//! This module re-exports the [`FailureClassifier`] trait and implementations
//! from [`tower_resilience_core::classifier`] for convenience.
//!
//! See the core module documentation for full details and examples.

pub use tower_resilience_core::classifier::{DefaultClassifier, FailureClassifier, FnClassifier};
