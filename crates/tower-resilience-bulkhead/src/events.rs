//! Event types for bulkhead pattern.

use std::time::{Duration, Instant};
use tower_resilience_core::events::ResilienceEvent;

/// Events emitted by the bulkhead pattern.
#[derive(Debug, Clone)]
pub enum BulkheadEvent {
    /// A call was permitted through the bulkhead.
    CallPermitted {
        /// Name of the bulkhead instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// Current number of concurrent calls.
        concurrent_calls: usize,
    },
    /// A call was rejected because the bulkhead is full.
    CallRejected {
        /// Name of the bulkhead instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// Maximum concurrent calls allowed.
        max_concurrent_calls: usize,
    },
    /// A call finished successfully.
    CallFinished {
        /// Name of the bulkhead instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// Duration of the call.
        duration: Duration,
    },
    /// A call finished with an error.
    CallFailed {
        /// Name of the bulkhead instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// Duration of the call.
        duration: Duration,
    },
}

impl ResilienceEvent for BulkheadEvent {
    fn event_type(&self) -> &'static str {
        match self {
            BulkheadEvent::CallPermitted { .. } => "call_permitted",
            BulkheadEvent::CallRejected { .. } => "call_rejected",
            BulkheadEvent::CallFinished { .. } => "call_finished",
            BulkheadEvent::CallFailed { .. } => "call_failed",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            BulkheadEvent::CallPermitted { timestamp, .. }
            | BulkheadEvent::CallRejected { timestamp, .. }
            | BulkheadEvent::CallFinished { timestamp, .. }
            | BulkheadEvent::CallFailed { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            BulkheadEvent::CallPermitted { pattern_name, .. }
            | BulkheadEvent::CallRejected { pattern_name, .. }
            | BulkheadEvent::CallFinished { pattern_name, .. }
            | BulkheadEvent::CallFailed { pattern_name, .. } => pattern_name,
        }
    }
}
