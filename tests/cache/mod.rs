//! Comprehensive tests for cache pattern.
//!
//! Test organization:
//! - cache_concurrency.rs: Thread safety and concurrent access
//! - cache_edge_cases.rs: Edge cases and stress testing
//! - cache_key_extraction.rs: Key extraction scenarios
//! - cache_layer.rs: Layer composition and Tower integration

mod cache_concurrency;
mod cache_edge_cases;
mod cache_key_extraction;
mod cache_layer;
