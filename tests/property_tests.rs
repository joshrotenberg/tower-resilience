//! Property-based tests for tower-resilience patterns.
//!
//! Run with: cargo test --test property_tests
//!
//! These tests use proptest to generate random inputs and verify that
//! key invariants hold across all resilience patterns.

mod property;
