//! Comprehensive tests for tower-resilience-hedge.
//!
//! This test suite provides coverage for the hedging pattern, organized into:
//!
//! - **integration**: Basic integration tests verifying core functionality
//! - **delay_modes**: Tests for latency mode vs parallel mode
//! - **events**: Tests for event emission and listeners
//! - **concurrency**: Tests for concurrent request handling

mod concurrency;
mod delay_modes;
mod events;
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
