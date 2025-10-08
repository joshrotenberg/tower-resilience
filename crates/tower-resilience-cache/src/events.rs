//! Event types for cache.

use std::time::Instant;
use tower_resilience_core::ResilienceEvent;

/// Events emitted by the cache.
#[derive(Debug, Clone)]
pub enum CacheEvent {
    /// A cache hit occurred.
    Hit {
        /// The name of the cache instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },
    /// A cache miss occurred.
    Miss {
        /// The name of the cache instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },
    /// An entry was evicted from the cache.
    Eviction {
        /// The name of the cache instance.
        pattern_name: String,
        /// When the event occurred.
        timestamp: Instant,
    },
}

impl ResilienceEvent for CacheEvent {
    fn event_type(&self) -> &'static str {
        match self {
            CacheEvent::Hit { .. } => "cache_hit",
            CacheEvent::Miss { .. } => "cache_miss",
            CacheEvent::Eviction { .. } => "cache_eviction",
        }
    }

    fn timestamp(&self) -> Instant {
        match self {
            CacheEvent::Hit { timestamp, .. }
            | CacheEvent::Miss { timestamp, .. }
            | CacheEvent::Eviction { timestamp, .. } => *timestamp,
        }
    }

    fn pattern_name(&self) -> &str {
        match self {
            CacheEvent::Hit { pattern_name, .. }
            | CacheEvent::Miss { pattern_name, .. }
            | CacheEvent::Eviction { pattern_name, .. } => pattern_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let now = Instant::now();

        let hit = CacheEvent::Hit {
            pattern_name: "test".to_string(),
            timestamp: now,
        };
        assert_eq!(hit.event_type(), "cache_hit");
        assert_eq!(hit.pattern_name(), "test");

        let miss = CacheEvent::Miss {
            pattern_name: "test".to_string(),
            timestamp: now,
        };
        assert_eq!(miss.event_type(), "cache_miss");

        let eviction = CacheEvent::Eviction {
            pattern_name: "test".to_string(),
            timestamp: now,
        };
        assert_eq!(eviction.event_type(), "cache_eviction");
    }
}
