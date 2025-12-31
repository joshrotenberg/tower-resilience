//! Comprehensive tests for tower-resilience-fallback.
//!
//! This test suite provides coverage for the fallback pattern, organized into:
//!
//! - **integration**: Basic integration tests verifying core functionality
//! - **strategies**: Tests for different fallback strategies
//! - **predicates**: Tests for selective error handling
//! - **composition**: Tests for composing fallback with other layers

mod composition;
mod integration;
mod predicates;
mod strategies;

use std::fmt;

/// Test error type for use in test services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestError {
    pub message: String,
    pub retryable: bool,
    pub code: u32,
}

impl TestError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            retryable: true,
            code: 500,
        }
    }

    pub fn non_retryable(message: &str) -> Self {
        Self {
            message: message.to_string(),
            retryable: false,
            code: 400,
        }
    }

    pub fn with_code(message: &str, code: u32) -> Self {
        Self {
            message: message.to_string(),
            retryable: true,
            code,
        }
    }
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestError({}): {}", self.code, self.message)
    }
}

impl std::error::Error for TestError {}
