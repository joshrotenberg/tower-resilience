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

    /// Registers a callback when a call is permitted through the bulkhead.
    ///
    /// This callback is invoked when a request successfully acquires a permit from the bulkhead
    /// and is allowed to proceed to the underlying service. The bulkhead permits calls as long
    /// as the current number of concurrent calls is below the configured maximum.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the current number of concurrent calls after this call was permitted.
    /// This value will be between 1 and `max_concurrent_calls` (inclusive).
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_bulkhead::BulkheadConfig;
    ///
    /// let config = BulkheadConfig::builder()
    ///     .max_concurrent_calls(10)
    ///     .on_call_permitted(|concurrent| {
    ///         println!("Call permitted - now {} concurrent calls", concurrent);
    ///         if concurrent >= 8 {
    ///             println!("Warning: approaching capacity!");
    ///         }
    ///     })
    ///     .build();
    /// ```
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

    /// Registers a callback when a call is rejected by the bulkhead.
    ///
    /// This callback is invoked when a request is rejected because the bulkhead is at full capacity
    /// (the maximum number of concurrent calls has been reached) and the request either cannot wait
    /// or has exceeded the `max_wait_duration`.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the configured maximum number of concurrent calls allowed.
    /// This represents the bulkhead's capacity that has been exceeded.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_bulkhead::BulkheadConfig;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let rejection_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&rejection_count);
    ///
    /// let config = BulkheadConfig::builder()
    ///     .max_concurrent_calls(25)
    ///     .on_call_rejected(move |max_capacity| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call rejected - bulkhead at capacity ({} max), total rejections: {}",
    ///                  max_capacity, count + 1);
    ///     })
    ///     .build();
    /// ```
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

    /// Registers a callback when a call finishes successfully.
    ///
    /// This callback is invoked when a request that was permitted through the bulkhead
    /// completes successfully and releases its permit. This happens regardless of the
    /// response value, as long as no error occurred.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the total duration the call took to complete,
    /// from when it was permitted until it finished.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_bulkhead::BulkheadConfig;
    /// use std::time::Duration;
    ///
    /// let config = BulkheadConfig::builder()
    ///     .max_concurrent_calls(25)
    ///     .on_call_finished(|duration| {
    ///         println!("Call completed successfully in {:?}", duration);
    ///         if duration > Duration::from_secs(5) {
    ///             println!("Warning: slow call detected");
    ///         }
    ///     })
    ///     .build();
    /// ```
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

    /// Registers a callback when a call fails with an error.
    ///
    /// This callback is invoked when a request that was permitted through the bulkhead
    /// fails with an error and releases its permit. The error could be from the underlying
    /// service or from middleware in the chain.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the total duration the call took before failing,
    /// from when it was permitted until the error occurred.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_bulkhead::BulkheadConfig;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let error_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&error_count);
    ///
    /// let config = BulkheadConfig::builder()
    ///     .max_concurrent_calls(25)
    ///     .on_call_failed(move |duration| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call failed after {:?} (total failures: {})", duration, count + 1);
    ///     })
    ///     .build();
    /// ```
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
