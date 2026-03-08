//! Integration tests for outlier detection.
//!
//! Test organization:
//! - integration.rs: Basic ejection lifecycle, recovery, and error propagation
//! - fleet.rs: Fleet-level behavior (max ejection percent, multi-instance)
//! - concurrency.rs: Concurrent access and stress tests

mod concurrency;
mod fleet;
mod integration;
