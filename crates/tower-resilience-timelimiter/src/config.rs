//! Configuration for time limiter.

use crate::events::TimeLimiterEvent;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Configuration for the time limiter pattern.
pub struct TimeLimiterConfig {
    pub(crate) timeout_duration: Duration,
    #[allow(dead_code)]
    pub(crate) cancel_running_future: bool,
    pub(crate) event_listeners: EventListeners<TimeLimiterEvent>,
    pub(crate) name: String,
}

impl TimeLimiterConfig {
    /// Creates a new configuration builder.
    pub fn builder() -> TimeLimiterConfigBuilder {
        TimeLimiterConfigBuilder::new()
    }
}

/// Builder for configuring and constructing a time limiter.
pub struct TimeLimiterConfigBuilder {
    timeout_duration: Duration,
    cancel_running_future: bool,
    event_listeners: EventListeners<TimeLimiterEvent>,
    name: String,
}

impl TimeLimiterConfigBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            timeout_duration: Duration::from_secs(5),
            cancel_running_future: false,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
        }
    }

    /// Sets the timeout duration.
    ///
    /// Default: 5 seconds
    pub fn timeout_duration(mut self, duration: Duration) -> Self {
        self.timeout_duration = duration;
        self
    }

    /// Sets whether to attempt to cancel the running future when a timeout occurs.
    ///
    /// When true, the future will be dropped on timeout, potentially canceling
    /// ongoing work. When false, the future continues running in the background
    /// but its result is ignored.
    ///
    /// Default: false
    pub fn cancel_running_future(mut self, cancel: bool) -> Self {
        self.cancel_running_future = cancel;
        self
    }

    /// Sets the name of this time limiter instance for observability.
    ///
    /// Default: `"<unnamed>"`
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Registers a callback when a call succeeds within the timeout.
    ///
    /// This callback is invoked when the underlying service call completes successfully
    /// before the configured timeout duration expires. This is the normal, happy-path case.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the actual duration the call took to complete.
    /// This will always be less than the configured `timeout_duration`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterConfig;
    /// use std::time::Duration;
    ///
    /// let config = TimeLimiterConfig::builder()
    ///     .timeout_duration(Duration::from_secs(5))
    ///     .on_success(|duration| {
    ///         println!("Call completed in {:?}", duration);
    ///         if duration > Duration::from_secs(4) {
    ///             println!("Warning: call took >80% of timeout");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_success<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let TimeLimiterEvent::Success { duration, .. } = event {
                f(*duration);
            }
        }));
        self
    }

    /// Registers a callback when a call fails with an error before the timeout.
    ///
    /// This callback is invoked when the underlying service call returns an error
    /// before the timeout expires. The error is not related to the timeout itself,
    /// but rather comes from the service or middleware in the chain.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the duration from when the call started until the error occurred.
    /// This will be less than the configured `timeout_duration`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterConfig;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let error_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&error_count);
    ///
    /// let config = TimeLimiterConfig::builder()
    ///     .timeout_duration(Duration::from_secs(5))
    ///     .on_error(move |duration| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call failed after {:?} (total errors: {})", duration, count + 1);
    ///     })
    ///     .build();
    /// ```
    pub fn on_error<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let TimeLimiterEvent::Error { duration, .. } = event {
                f(*duration);
            }
        }));
        self
    }

    /// Registers a callback when a call exceeds the timeout duration.
    ///
    /// This callback is invoked when the underlying service call does not complete
    /// within the configured timeout duration. The call will be cancelled (if
    /// `cancel_running_future` is true) or allowed to continue in the background,
    /// and a timeout error will be returned to the caller.
    ///
    /// # Callback Signature
    /// `Fn()` - Called with no parameters when a timeout occurs.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterConfig;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let timeout_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&timeout_count);
    ///
    /// let config = TimeLimiterConfig::builder()
    ///     .timeout_duration(Duration::from_secs(5))
    ///     .cancel_running_future(true)
    ///     .on_timeout(move || {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call timed out (total timeouts: {})", count + 1);
    ///         if count > 10 {
    ///             println!("WARNING: High timeout rate detected!");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_timeout<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, TimeLimiterEvent::Timeout { .. }) {
                f();
            }
        }));
        self
    }

    /// Builds the time limiter layer.
    pub fn build(self) -> crate::TimeLimiterLayer {
        let config = TimeLimiterConfig {
            timeout_duration: self.timeout_duration,
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::TimeLimiterLayer::new(config)
    }
}

impl Default for TimeLimiterConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let _layer = TimeLimiterConfig::builder().build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = TimeLimiterConfig::builder()
            .timeout_duration(Duration::from_millis(100))
            .cancel_running_future(true)
            .name("my-timelimiter")
            .build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_event_listeners() {
        let _layer = TimeLimiterConfig::builder()
            .on_success(|_| {})
            .on_error(|_| {})
            .on_timeout(|| {})
            .build();
        // If this compiles and doesn't panic, the event listener registration works
    }
}
