//! Stress tests for tower-resilience patterns.
//!
//! These tests push the patterns to their limits to validate behavior under extreme conditions.
//! They are marked with `#[ignore]` and must be run explicitly:
//!
//! ```bash
//! # Run all stress tests
//! cargo test --test stress -- --ignored
//!
//! # Run specific stress test module
//! cargo test --test stress circuitbreaker -- --ignored
//!
//! # Run with output
//! cargo test --test stress -- --ignored --nocapture
//! ```

#[path = "stress/mod.rs"]
mod stress;
