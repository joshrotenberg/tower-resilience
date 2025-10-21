//! Bulkhead metrics regression tests
//!
//! Note: Bulkhead metrics tests are complex due to trait bounds on Service cloning.
//! See tests/bulkhead/integration.rs for comprehensive bulkhead testing.
//! The metrics emitted are: bulkhead_calls_permitted_total, bulkhead_calls_rejected_total,
//! bulkhead_calls_finished_total, bulkhead_calls_failed_total, bulkhead_concurrent_calls,
//! bulkhead_wait_duration_seconds, bulkhead_call_duration_seconds
