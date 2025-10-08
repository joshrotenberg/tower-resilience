//! Event types for time limiter.

use std::time::{Duration, Instant};
use tower_resilience_core::ResilienceEvent;

/// Events emitted by the time limiter.
#[derive(Debug, Clone)]
pub enum TimeLimiterEvent {
    /// A call completed successfully within the timeout.
    Success {
        /// The name of the time limiter instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// How long the call took.
        duration: Duration,
    },
    /// A call failed with an error.
    Error {
        /// The name of the time limiter instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// How long before the error occurred.
        duration: Duration,
    },
    /// A call timed out.
    Timeout {
        /// The name of the time limiter instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
        /// The configured timeout duration.
        timeout_duration: Duration,
    },
}

impl ResilienceEvent for TimeLimiterEvent {
    fn event_type(&self) -> &'static str {
        match self {
            TimeLimiterEvent::Success { .. } => "success",
            TimeLimiterEvent::Error { .. } => "error",
            TimeLimiterEvent::Timeout { .. } => "timeout",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            TimeLimiterEvent::Success { timestamp, .. }
            | TimeLimiterEvent::Error { timestamp, .. }
            | TimeLimiterEvent::Timeout { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            TimeLimiterEvent::Success { pattern_name, .. }
            | TimeLimiterEvent::Error { pattern_name, .. }
            | TimeLimiterEvent::Timeout { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();
        let success = TimeLimiterEvent::Success {
            pattern_name: "test".to_string(),
            timestamp: now,
            duration: Duration::from_millis(100),
        };
        assert_eq!(success.event_type(), "success");
        assert_eq!(success.pattern_name(), "test");

        let error = TimeLimiterEvent::Error {
            pattern_name: "test".to_string(),
            timestamp: now,
            duration: Duration::from_millis(50),
        };
        assert_eq!(error.event_type(), "error");

        let timeout = TimeLimiterEvent::Timeout {
            pattern_name: "test".to_string(),
            timestamp: now,
            timeout_duration: Duration::from_secs(5),
        };
        assert_eq!(timeout.event_type(), "timeout");
    }
}
