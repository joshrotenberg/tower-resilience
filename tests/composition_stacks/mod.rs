//! Composition stack compile tests.
//!
//! These tests verify that the example stacks from the composition guide
//! (crates/tower-resilience/src/composition.rs) compile correctly.
//!
//! Each test corresponds to a stack example in the documentation.
//! The tests are intentionally simple - they just verify the layers compose
//! without type errors. Runtime behavior is tested elsewhere.

// Allow dead code - these structs exist only for type checking compilation
#![allow(dead_code)]

mod caching;
mod database;
mod external_api;
mod latency_critical;
mod message_queues;
mod microservices;
mod server_side;
