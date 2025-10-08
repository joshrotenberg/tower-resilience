use std::time::{Duration, Instant};
use tower_resilience_core::events::ResilienceEvent;

/// Events emitted by the rate limiter middleware.
#[derive(Debug, Clone)]
pub enum RateLimiterEvent {
    /// A permit was successfully acquired.
    PermitAcquired {
        pattern_name: String,
        timestamp: Instant,
        wait_duration: Duration,
    },
    /// A request was rejected due to rate limit.
    PermitRejected {
        pattern_name: String,
        timestamp: Instant,
        timeout_duration: Duration,
    },
    /// Permits were refreshed.
    PermitsRefreshed {
        pattern_name: String,
        timestamp: Instant,
        available_permits: usize,
    },
}

impl ResilienceEvent for RateLimiterEvent {
    fn event_type(&self) -> &'static str {
        match self {
            RateLimiterEvent::PermitAcquired { .. } => "PermitAcquired",
            RateLimiterEvent::PermitRejected { .. } => "PermitRejected",
            RateLimiterEvent::PermitsRefreshed { .. } => "PermitsRefreshed",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            RateLimiterEvent::PermitAcquired { timestamp, .. } => *timestamp,
            RateLimiterEvent::PermitRejected { timestamp, .. } => *timestamp,
            RateLimiterEvent::PermitsRefreshed { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            RateLimiterEvent::PermitAcquired { pattern_name, .. } => pattern_name,
            RateLimiterEvent::PermitRejected { pattern_name, .. } => pattern_name,
            RateLimiterEvent::PermitsRefreshed { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();

        let acquired = RateLimiterEvent::PermitAcquired {
            pattern_name: "test".to_string(),
            timestamp: now,
            wait_duration: Duration::from_millis(10),
        };
        assert_eq!(acquired.event_type(), "PermitAcquired");

        let rejected = RateLimiterEvent::PermitRejected {
            pattern_name: "test".to_string(),
            timestamp: now,
            timeout_duration: Duration::from_secs(1),
        };
        assert_eq!(rejected.event_type(), "PermitRejected");

        let refreshed = RateLimiterEvent::PermitsRefreshed {
            pattern_name: "test".to_string(),
            timestamp: now,
            available_permits: 10,
        };
        assert_eq!(refreshed.event_type(), "PermitsRefreshed");
    }
}
