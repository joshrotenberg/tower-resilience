//! Integration tests for the weighted router.
//!
//! Test organization:
//! - integration.rs: Basic routing, distribution, and error propagation
//! - composition.rs: Composing router with other resilience patterns
//! - concurrency.rs: Concurrent access and stress tests

mod composition;
mod concurrency;
mod integration;
