use crate::CircuitState;
use std::time::Instant;
use tower_resilience_core::ResilienceEvent;

/// Events emitted by the circuit breaker pattern.
#[derive(Debug, Clone)]
pub enum CircuitBreakerEvent {
    /// A call was permitted through the circuit breaker.
    CallPermitted {
        pattern_name: String,
        timestamp: Instant,
        state: CircuitState,
    },
    /// A call was rejected because the circuit is open.
    CallRejected {
        pattern_name: String,
        timestamp: Instant,
    },
    /// The circuit breaker transitioned between states.
    StateTransition {
        pattern_name: String,
        timestamp: Instant,
        from_state: CircuitState,
        to_state: CircuitState,
    },
    /// A successful call was recorded.
    SuccessRecorded {
        pattern_name: String,
        timestamp: Instant,
        state: CircuitState,
    },
    /// A failed call was recorded.
    FailureRecorded {
        pattern_name: String,
        timestamp: Instant,
        state: CircuitState,
    },
}

impl ResilienceEvent for CircuitBreakerEvent {
    fn event_type(&self) -> &'static str {
        match self {
            CircuitBreakerEvent::CallPermitted { .. } => "call_permitted",
            CircuitBreakerEvent::CallRejected { .. } => "call_rejected",
            CircuitBreakerEvent::StateTransition { .. } => "state_transition",
            CircuitBreakerEvent::SuccessRecorded { .. } => "success_recorded",
            CircuitBreakerEvent::FailureRecorded { .. } => "failure_recorded",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            CircuitBreakerEvent::CallPermitted { timestamp, .. }
            | CircuitBreakerEvent::CallRejected { timestamp, .. }
            | CircuitBreakerEvent::StateTransition { timestamp, .. }
            | CircuitBreakerEvent::SuccessRecorded { timestamp, .. }
            | CircuitBreakerEvent::FailureRecorded { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            CircuitBreakerEvent::CallPermitted { pattern_name, .. }
            | CircuitBreakerEvent::CallRejected { pattern_name, .. }
            | CircuitBreakerEvent::StateTransition { pattern_name, .. }
            | CircuitBreakerEvent::SuccessRecorded { pattern_name, .. }
            | CircuitBreakerEvent::FailureRecorded { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();
        let name = "test".to_string();

        let call_permitted = CircuitBreakerEvent::CallPermitted {
            pattern_name: name.clone(),
            timestamp: now,
            state: CircuitState::Closed,
        };
        assert_eq!(call_permitted.event_type(), "call_permitted");
        assert_eq!(call_permitted.pattern_name(), "test");

        let call_rejected = CircuitBreakerEvent::CallRejected {
            pattern_name: name.clone(),
            timestamp: now,
        };
        assert_eq!(call_rejected.event_type(), "call_rejected");

        let state_transition = CircuitBreakerEvent::StateTransition {
            pattern_name: name.clone(),
            timestamp: now,
            from_state: CircuitState::Closed,
            to_state: CircuitState::Open,
        };
        assert_eq!(state_transition.event_type(), "state_transition");

        let success = CircuitBreakerEvent::SuccessRecorded {
            pattern_name: name.clone(),
            timestamp: now,
            state: CircuitState::Closed,
        };
        assert_eq!(success.event_type(), "success_recorded");

        let failure = CircuitBreakerEvent::FailureRecorded {
            pattern_name: name,
            timestamp: now,
            state: CircuitState::Closed,
        };
        assert_eq!(failure.event_type(), "failure_recorded");
    }

    #[test]
    fn test_event_timestamp() {
        let now = Instant::now();
        let event = CircuitBreakerEvent::CallPermitted {
            pattern_name: "test".to_string(),
            timestamp: now,
            state: CircuitState::Closed,
        };
        assert_eq!(event.timestamp(), now);
    }
}
