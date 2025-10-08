//! Comprehensive tests for bulkhead pattern.
//!
//! Test organization:
//! - integration.rs: Basic integration tests
//! - concurrency.rs: P0 - Concurrent request handling
//! - config.rs: P0 - Configuration validation
//! - permits.rs: P0 - Permit lifecycle management
//! - timeout.rs: P0 - Timeout edge cases

mod concurrency;
mod config;
mod integration;
mod permits;
mod timeout;
