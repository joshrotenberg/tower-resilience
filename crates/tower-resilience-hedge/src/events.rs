//! Events emitted by the hedging middleware.

use std::time::{Duration, Instant};
use tower_resilience_core::ResilienceEvent;

/// Events emitted during hedge execution.
#[derive(Debug, Clone)]
pub enum HedgeEvent {
    /// Primary request started.
    PrimaryStarted {
        /// Name of the hedge instance.
        name: Option<String>,
        /// When this event occurred.
        timestamp: Instant,
    },

    /// A hedge attempt was started.
    HedgeStarted {
        /// Name of the hedge instance.
        name: Option<String>,
        /// Which hedge attempt (1-indexed).
        attempt: usize,
        /// Delay that elapsed before this hedge was fired.
        delay: Duration,
        /// When this event occurred.
        timestamp: Instant,
    },

    /// Primary request completed successfully first.
    PrimarySucceeded {
        /// Name of the hedge instance.
        name: Option<String>,
        /// Total duration from start to success.
        duration: Duration,
        /// Number of hedge requests that were cancelled.
        hedges_cancelled: usize,
        /// When this event occurred.
        timestamp: Instant,
    },

    /// A hedge request completed successfully first.
    HedgeSucceeded {
        /// Name of the hedge instance.
        name: Option<String>,
        /// Which hedge attempt succeeded (1-indexed).
        attempt: usize,
        /// Total duration from start to success.
        duration: Duration,
        /// Whether the primary request was cancelled.
        primary_cancelled: bool,
        /// When this event occurred.
        timestamp: Instant,
    },

    /// All attempts (primary and hedges) failed.
    AllFailed {
        /// Name of the hedge instance.
        name: Option<String>,
        /// Total number of attempts made.
        attempts: usize,
        /// When this event occurred.
        timestamp: Instant,
    },
}

impl ResilienceEvent for HedgeEvent {
    fn event_type(&self) -> &'static str {
        match self {
            HedgeEvent::PrimaryStarted { .. } => "primary_started",
            HedgeEvent::HedgeStarted { .. } => "hedge_started",
            HedgeEvent::PrimarySucceeded { .. } => "primary_succeeded",
            HedgeEvent::HedgeSucceeded { .. } => "hedge_succeeded",
            HedgeEvent::AllFailed { .. } => "all_failed",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            HedgeEvent::PrimaryStarted { timestamp, .. } => *timestamp,
            HedgeEvent::HedgeStarted { timestamp, .. } => *timestamp,
            HedgeEvent::PrimarySucceeded { timestamp, .. } => *timestamp,
            HedgeEvent::HedgeSucceeded { timestamp, .. } => *timestamp,
            HedgeEvent::AllFailed { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            HedgeEvent::PrimaryStarted { name, .. } => name.as_deref().unwrap_or("hedge"),
            HedgeEvent::HedgeStarted { name, .. } => name.as_deref().unwrap_or("hedge"),
            HedgeEvent::PrimarySucceeded { name, .. } => name.as_deref().unwrap_or("hedge"),
            HedgeEvent::HedgeSucceeded { name, .. } => name.as_deref().unwrap_or("hedge"),
            HedgeEvent::AllFailed { name, .. } => name.as_deref().unwrap_or("hedge"),
        }
    }
}
