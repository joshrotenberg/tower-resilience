//! Events emitted by the fallback service.

use std::time::Instant;
use tower_resilience_core::ResilienceEvent;

/// Events emitted by the fallback service.
#[derive(Debug, Clone)]
pub enum FallbackEvent {
    /// The inner service succeeded; no fallback was needed.
    Success {
        /// Name of the fallback instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },

    /// The inner service failed; fallback will be attempted.
    FailedAttempt {
        /// Name of the fallback instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },

    /// The fallback was successfully applied.
    Applied {
        /// Name of the fallback instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// The strategy that was applied.
        strategy: &'static str,
    },

    /// The fallback itself failed (only possible with service fallback).
    Failed {
        /// Name of the fallback instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },

    /// The error didn't match the predicate; propagated as-is.
    Skipped {
        /// Name of the fallback instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },
}

impl ResilienceEvent for FallbackEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Success { .. } => "success",
            Self::FailedAttempt { .. } => "failed_attempt",
            Self::Applied { .. } => "applied",
            Self::Failed { .. } => "failed",
            Self::Skipped { .. } => "skipped",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            Self::Success { timestamp, .. }
            | Self::FailedAttempt { timestamp, .. }
            | Self::Applied { timestamp, .. }
            | Self::Failed { timestamp, .. }
            | Self::Skipped { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            Self::Success { pattern_name, .. }
            | Self::FailedAttempt { pattern_name, .. }
            | Self::Applied { pattern_name, .. }
            | Self::Failed { pattern_name, .. }
            | Self::Skipped { pattern_name, .. } => pattern_name,
        }
    }
}
