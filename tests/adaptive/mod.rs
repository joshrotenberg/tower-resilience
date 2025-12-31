//! Comprehensive tests for tower-resilience-adaptive.
//!
//! This test suite provides coverage for the adaptive concurrency limiter:
//!
//! - **integration**: Basic integration tests verifying core functionality
//! - **algorithms**: Tests for AIMD and Vegas algorithms
//! - **concurrency**: Tests for concurrent request handling

mod algorithms;
mod concurrency;
mod integration;
