//! Events emitted by the outlier detection middleware.

use std::time::{Duration, Instant};
use tower_resilience_core::events::ResilienceEvent;

/// Events emitted by the outlier detection middleware.
#[derive(Debug, Clone)]
pub enum OutlierDetectionEvent {
    /// An instance has been ejected.
    Ejected {
        /// The pattern name.
        pattern_name: String,
        /// When the ejection occurred.
        timestamp: Instant,
        /// The name of the ejected instance.
        instance_name: String,
        /// The number of consecutive errors that triggered ejection.
        consecutive_errors: usize,
        /// How long the instance will be ejected.
        ejection_duration: Duration,
    },
    /// An instance has recovered from ejection.
    Recovered {
        /// The pattern name.
        pattern_name: String,
        /// When recovery occurred.
        timestamp: Instant,
        /// The name of the recovered instance.
        instance_name: String,
        /// How long the instance was ejected.
        ejected_duration: Duration,
    },
    /// A request was rejected because the instance is ejected.
    Rejected {
        /// The pattern name.
        pattern_name: String,
        /// When the rejection occurred.
        timestamp: Instant,
        /// The name of the ejected instance.
        instance_name: String,
    },
    /// An ejection was skipped because max_ejection_percent would be exceeded.
    EjectionSkipped {
        /// The pattern name.
        pattern_name: String,
        /// When the skip occurred.
        timestamp: Instant,
        /// The name of the instance that would have been ejected.
        instance_name: String,
        /// Current ejection percentage.
        current_ejection_percent: f64,
    },
}

impl ResilienceEvent for OutlierDetectionEvent {
    fn event_type(&self) -> &'static str {
        match self {
            OutlierDetectionEvent::Ejected { .. } => "ejected",
            OutlierDetectionEvent::Recovered { .. } => "recovered",
            OutlierDetectionEvent::Rejected { .. } => "rejected",
            OutlierDetectionEvent::EjectionSkipped { .. } => "ejection_skipped",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            OutlierDetectionEvent::Ejected { timestamp, .. }
            | OutlierDetectionEvent::Recovered { timestamp, .. }
            | OutlierDetectionEvent::Rejected { timestamp, .. }
            | OutlierDetectionEvent::EjectionSkipped { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            OutlierDetectionEvent::Ejected { pattern_name, .. }
            | OutlierDetectionEvent::Recovered { pattern_name, .. }
            | OutlierDetectionEvent::Rejected { pattern_name, .. }
            | OutlierDetectionEvent::EjectionSkipped { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();

        let ejected = OutlierDetectionEvent::Ejected {
            pattern_name: "test".to_string(),
            timestamp: now,
            instance_name: "backend-1".to_string(),
            consecutive_errors: 5,
            ejection_duration: Duration::from_secs(30),
        };
        assert_eq!(ejected.event_type(), "ejected");
        assert_eq!(ejected.pattern_name(), "test");

        let recovered = OutlierDetectionEvent::Recovered {
            pattern_name: "test".to_string(),
            timestamp: now,
            instance_name: "backend-1".to_string(),
            ejected_duration: Duration::from_secs(30),
        };
        assert_eq!(recovered.event_type(), "recovered");

        let rejected = OutlierDetectionEvent::Rejected {
            pattern_name: "test".to_string(),
            timestamp: now,
            instance_name: "backend-1".to_string(),
        };
        assert_eq!(rejected.event_type(), "rejected");

        let skipped = OutlierDetectionEvent::EjectionSkipped {
            pattern_name: "test".to_string(),
            timestamp: now,
            instance_name: "backend-1".to_string(),
            current_ejection_percent: 50.0,
        };
        assert_eq!(skipped.event_type(), "ejection_skipped");
    }
}
