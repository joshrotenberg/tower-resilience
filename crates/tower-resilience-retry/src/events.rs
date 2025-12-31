use std::time::{Duration, Instant};
use tower_resilience_core::events::ResilienceEvent;

/// Events emitted by the retry middleware.
#[derive(Debug, Clone)]
pub enum RetryEvent {
    /// A retry attempt is about to be made.
    Retry {
        pattern_name: String,
        timestamp: Instant,
        attempt: usize,
        delay: Duration,
    },
    /// The operation succeeded (either on first try or after retries).
    Success {
        pattern_name: String,
        timestamp: Instant,
        attempts: usize,
    },
    /// The operation failed after exhausting all retry attempts.
    Error {
        pattern_name: String,
        timestamp: Instant,
        attempts: usize,
    },
    /// An error occurred but was not retried (filtered by retry predicate).
    IgnoredError {
        pattern_name: String,
        timestamp: Instant,
    },
    /// A retry was skipped because the retry budget was exhausted.
    BudgetExhausted {
        pattern_name: String,
        timestamp: Instant,
        attempt: usize,
    },
}

impl ResilienceEvent for RetryEvent {
    fn event_type(&self) -> &'static str {
        match self {
            RetryEvent::Retry { .. } => "Retry",
            RetryEvent::Success { .. } => "Success",
            RetryEvent::Error { .. } => "Error",
            RetryEvent::IgnoredError { .. } => "IgnoredError",
            RetryEvent::BudgetExhausted { .. } => "BudgetExhausted",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            RetryEvent::Retry { timestamp, .. }
            | RetryEvent::Success { timestamp, .. }
            | RetryEvent::Error { timestamp, .. }
            | RetryEvent::IgnoredError { timestamp, .. }
            | RetryEvent::BudgetExhausted { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            RetryEvent::Retry { pattern_name, .. }
            | RetryEvent::Success { pattern_name, .. }
            | RetryEvent::Error { pattern_name, .. }
            | RetryEvent::IgnoredError { pattern_name, .. }
            | RetryEvent::BudgetExhausted { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();
        let retry = RetryEvent::Retry {
            pattern_name: "test".to_string(),
            timestamp: now,
            attempt: 1,
            delay: Duration::from_secs(1),
        };
        assert_eq!(retry.event_type(), "Retry");

        let success = RetryEvent::Success {
            pattern_name: "test".to_string(),
            timestamp: now,
            attempts: 2,
        };
        assert_eq!(success.event_type(), "Success");

        let error = RetryEvent::Error {
            pattern_name: "test".to_string(),
            timestamp: now,
            attempts: 3,
        };
        assert_eq!(error.event_type(), "Error");

        let ignored = RetryEvent::IgnoredError {
            pattern_name: "test".to_string(),
            timestamp: now,
        };
        assert_eq!(ignored.event_type(), "IgnoredError");
    }
}
