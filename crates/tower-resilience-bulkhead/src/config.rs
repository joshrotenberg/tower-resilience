//! Configuration for the bulkhead pattern.

use crate::events::BulkheadEvent;
use std::time::Duration;
use tower_resilience_core::events::{EventListeners, FnListener};

/// Configuration for the bulkhead pattern.
#[derive(Clone)]
pub struct BulkheadConfig {
    /// Maximum number of concurrent calls allowed.
    pub(crate) max_concurrent_calls: usize,
    /// Maximum time to wait for a permit.
    pub(crate) max_wait_duration: Option<Duration>,
    /// Name of this bulkhead instance.
    pub(crate) name: String,
    /// Event listeners.
    pub(crate) event_listeners: EventListeners<BulkheadEvent>,
}

impl BulkheadConfig {
    /// Creates a new configuration builder.
    pub fn builder() -> BulkheadConfigBuilder {
        BulkheadConfigBuilder::new()
    }
}

/// Builder for bulkhead configuration.
pub struct BulkheadConfigBuilder {
    max_concurrent_calls: usize,
    max_wait_duration: Option<Duration>,
    name: String,
    event_listeners: EventListeners<BulkheadEvent>,
}

impl BulkheadConfigBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            max_concurrent_calls: 25,
            max_wait_duration: None,
            name: "bulkhead".to_string(),
            event_listeners: EventListeners::new(),
        }
    }

    /// Sets the maximum number of concurrent calls.
    ///
    /// Default: 25
    pub fn max_concurrent_calls(mut self, max: usize) -> Self {
        self.max_concurrent_calls = max;
        self
    }

    /// Sets the maximum time to wait for a permit.
    ///
    /// If `None`, calls will wait indefinitely.
    /// Default: None
    pub fn max_wait_duration(mut self, duration: Option<Duration>) -> Self {
        self.max_wait_duration = duration;
        self
    }

    /// Sets the name of this bulkhead instance.
    ///
    /// Default: "bulkhead"
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Registers a callback for when a call is permitted.
    pub fn on_call_permitted<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let BulkheadEvent::CallPermitted {
                concurrent_calls, ..
            } = event
            {
                f(*concurrent_calls);
            }
        }));
        self
    }

    /// Registers a callback for when a call is rejected.
    pub fn on_call_rejected<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let BulkheadEvent::CallRejected {
                max_concurrent_calls,
                ..
            } = event
            {
                f(*max_concurrent_calls);
            }
        }));
        self
    }

    /// Registers a callback for when a call finishes successfully.
    pub fn on_call_finished<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let BulkheadEvent::CallFinished { duration, .. } = event {
                f(*duration);
            }
        }));
        self
    }

    /// Registers a callback for when a call fails.
    pub fn on_call_failed<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let BulkheadEvent::CallFailed { duration, .. } = event {
                f(*duration);
            }
        }));
        self
    }

    /// Builds the configuration and returns a BulkheadLayer.
    pub fn build(self) -> crate::layer::BulkheadLayer {
        let config = BulkheadConfig {
            max_concurrent_calls: self.max_concurrent_calls,
            max_wait_duration: self.max_wait_duration,
            name: self.name,
            event_listeners: self.event_listeners,
        };
        crate::layer::BulkheadLayer::new(config)
    }
}

impl Default for BulkheadConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
