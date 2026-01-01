//! Comprehensive tests for tower-resilience-coalesce.
//!
//! This test suite provides coverage for the coalesce (singleflight) pattern:
//!
//! - **integration**: Basic integration tests verifying core functionality
//! - **concurrency**: Tests for concurrent request coalescing
//! - **errors**: Tests for error propagation to all waiters

mod concurrency;
mod integration;

use std::fmt;

/// Test error type for use in test services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestError {
    pub message: String,
}

impl TestError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestError: {}", self.message)
    }
}

impl std::error::Error for TestError {}
