//! Comprehensive tests for tower-timelimiter.
//!
//! This test suite provides P0 coverage for the time limiter pattern, organized into:
//!
//! - **integration**: Basic integration tests verifying core functionality
//! - **timeout_precision**: Tests for timeout accuracy and edge cases
//! - **cancellation**: Tests for future cancellation behavior
//! - **concurrency**: Tests for concurrent timeout handling
//! - **config**: Tests for configuration validation

mod cancellation;
mod concurrency;
mod config;
mod integration;
mod timeout_precision;

use std::fmt;

/// Test error type for use in test services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestError(pub String);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestError: {}", self.0)
    }
}

impl std::error::Error for TestError {}
