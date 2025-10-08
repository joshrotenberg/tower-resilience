//! Comprehensive tests for circuit breaker pattern.
//!
//! Test organization:
//! - integration.rs: Basic integration tests
//! - concurrency.rs: P0 - Concurrent access patterns
//! - config_validation.rs: P0 - Configuration edge cases
//! - thresholds.rs: P0 - Threshold precision testing
//! - time_based.rs: P0 - Time-based window behavior
//! - combinations.rs: P1 - Feature combinations
//! - half_open.rs: P1 - Half-open state complexity
//! - reset.rs: P1 - Reset functionality
//! - edge_cases.rs: P2 - Event listeners, failure classifiers

mod combinations;
mod concurrency;
mod config_validation;
mod edge_cases;
mod half_open;
mod integration;
mod reset;
mod thresholds;
mod time_based;
