//! Event types for the weighted router.

use std::time::Instant;
use tower_resilience_core::events::ResilienceEvent;

/// Events emitted by the weighted router.
#[derive(Debug, Clone)]
pub enum RouterEvent {
    /// A request was routed to a backend.
    RequestRouted {
        /// Name of the router instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// Index of the selected backend.
        backend_index: usize,
        /// Weight of the selected backend.
        backend_weight: u32,
    },
}

impl ResilienceEvent for RouterEvent {
    fn event_type(&self) -> &'static str {
        match self {
            RouterEvent::RequestRouted { .. } => "request_routed",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            RouterEvent::RequestRouted { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            RouterEvent::RequestRouted { pattern_name, .. } => pattern_name,
        }
    }
}
