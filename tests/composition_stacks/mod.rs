//! Composition stack compile tests.
//!
//! These tests verify that the example stacks from the composition guide
//! (crates/tower-resilience/src/composition.rs) compile correctly.
//!
//! Each test corresponds to a stack example in the documentation.
//! The compile tests are intentionally simple - they just verify the layers
//! compose without type errors.
//!
//! The `order_verification` module contains runtime tests that verify
//! layer ordering actually affects behavior as documented.

// Allow dead code - these structs exist only for type checking compilation
#![allow(dead_code)]

mod caching;
mod database;
mod external_api;
mod latency_critical;
mod message_queues;
mod microservices;
mod order_verification;
mod server_side;
mod test_utils;
