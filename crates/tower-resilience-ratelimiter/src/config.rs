use crate::events::RateLimiterEvent;
use std::time::Duration;
use tower_resilience_core::events::{EventListeners, FnListener};

/// Configuration for the rate limiter pattern.
pub struct RateLimiterConfig {
    pub(crate) limit_for_period: usize,
    pub(crate) refresh_period: Duration,
    pub(crate) timeout_duration: Duration,
    pub(crate) event_listeners: EventListeners<RateLimiterEvent>,
    pub(crate) name: String,
}

impl RateLimiterConfig {
    /// Creates a new builder for rate limiter configuration.
    pub fn builder() -> RateLimiterConfigBuilder {
        RateLimiterConfigBuilder::new()
    }
}

/// Builder for [`RateLimiterConfig`].
pub struct RateLimiterConfigBuilder {
    limit_for_period: usize,
    refresh_period: Duration,
    timeout_duration: Duration,
    event_listeners: EventListeners<RateLimiterEvent>,
    name: String,
}

impl Default for RateLimiterConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiterConfigBuilder {
    /// Creates a new builder with defaults.
    ///
    /// Defaults:
    /// - limit_for_period: 50
    /// - refresh_period: 1 second
    /// - timeout_duration: 100ms
    /// - name: `"<unnamed>"`
    pub fn new() -> Self {
        Self {
            limit_for_period: 50,
            refresh_period: Duration::from_secs(1),
            timeout_duration: Duration::from_millis(100),
            event_listeners: EventListeners::new(),
            name: "<unnamed>".to_string(),
        }
    }

    /// Sets the maximum number of permits available per refresh period.
    ///
    /// This is the core rate limiting parameter - for example, setting this to 100
    /// with a refresh_period of 1 second allows 100 requests per second.
    pub fn limit_for_period(mut self, limit: usize) -> Self {
        self.limit_for_period = limit;
        self
    }

    /// Sets the duration of the refresh period.
    ///
    /// After each period, the available permits are reset to limit_for_period.
    pub fn refresh_period(mut self, duration: Duration) -> Self {
        self.refresh_period = duration;
        self
    }

    /// Sets how long to wait for a permit before rejecting the request.
    ///
    /// If a permit is not available immediately, the rate limiter will wait
    /// up to this duration for the next refresh period. If the wait would
    /// exceed this timeout, the request is rejected immediately.
    pub fn timeout_duration(mut self, duration: Duration) -> Self {
        self.timeout_duration = duration;
        self
    }

    /// Sets the name for this rate limiter instance (used in events).
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = name.into();
        self
    }

    /// Registers a callback for permit acquired events.
    pub fn on_permit_acquired<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitAcquired { wait_duration, .. } = event {
                f(*wait_duration);
            }
        }));
        self
    }

    /// Registers a callback for permit rejected events.
    pub fn on_permit_rejected<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitRejected {
                timeout_duration, ..
            } = event
            {
                f(*timeout_duration);
            }
        }));
        self
    }

    /// Registers a callback for permits refreshed events.
    pub fn on_permits_refreshed<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RateLimiterEvent::PermitsRefreshed {
                available_permits, ..
            } = event
            {
                f(*available_permits);
            }
        }));
        self
    }

    /// Builds the rate limiter layer.
    pub fn build(self) -> crate::RateLimiterLayer {
        let config = RateLimiterConfig {
            limit_for_period: self.limit_for_period,
            refresh_period: self.refresh_period,
            timeout_duration: self.timeout_duration,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::RateLimiterLayer::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let _layer = RateLimiterConfig::builder().build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = RateLimiterConfig::builder()
            .limit_for_period(100)
            .refresh_period(Duration::from_secs(2))
            .timeout_duration(Duration::from_millis(500))
            .name("test-limiter")
            .build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_event_listeners() {
        let _layer = RateLimiterConfig::builder()
            .on_permit_acquired(|_| {})
            .on_permit_rejected(|_| {})
            .build();
        // If this compiles and doesn't panic, the event listener registration works
    }
}
