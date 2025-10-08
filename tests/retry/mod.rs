//! Comprehensive tests for retry pattern.
//!
//! Test organization:
//! - retry_backoff.rs: Backoff strategy tests
//! - retry_predicates.rs: Retry predicate filtering tests
//! - retry_behavior.rs: Core retry logic tests
//! - retry_events.rs: Event system tests
//! - retry_config.rs: Configuration and builder tests

mod retry_backoff;
mod retry_behavior;
mod retry_config;
mod retry_events;
mod retry_predicates;
