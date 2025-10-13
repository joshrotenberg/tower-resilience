//! Event types for chaos engineering layer.

use std::time::{Duration, Instant};
use tower_resilience_core::ResilienceEvent;

/// Events emitted by the chaos layer.
#[derive(Debug, Clone)]
pub enum ChaosEvent {
    /// An error was injected into the response.
    ErrorInjected {
        /// Name of the chaos layer instance
        pattern_name: String,
        /// When the event occurred
        timestamp: Instant,
    },
    /// Latency was injected (request delayed).
    LatencyInjected {
        /// Name of the chaos layer instance
        pattern_name: String,
        /// When the event occurred
        timestamp: Instant,
        /// Amount of delay injected
        delay: Duration,
    },
    /// Request passed through without chaos injection.
    PassedThrough {
        /// Name of the chaos layer instance
        pattern_name: String,
        /// When the event occurred
        timestamp: Instant,
    },
}

impl ResilienceEvent for ChaosEvent {
    fn event_type(&self) -> &'static str {
        match self {
            ChaosEvent::ErrorInjected { .. } => "chaos.error_injected",
            ChaosEvent::LatencyInjected { .. } => "chaos.latency_injected",
            ChaosEvent::PassedThrough { .. } => "chaos.passed_through",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            ChaosEvent::ErrorInjected { timestamp, .. }
            | ChaosEvent::LatencyInjected { timestamp, .. }
            | ChaosEvent::PassedThrough { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            ChaosEvent::ErrorInjected { pattern_name, .. }
            | ChaosEvent::LatencyInjected { pattern_name, .. }
            | ChaosEvent::PassedThrough { pattern_name, .. } => pattern_name,
        }
    }
}
