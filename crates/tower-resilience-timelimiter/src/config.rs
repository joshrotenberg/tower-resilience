//! Configuration for time limiter.

use crate::events::TimeLimiterEvent;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Source for determining timeout duration.
///
/// This enum allows configuring either a fixed timeout for all requests
/// or a dynamic timeout extracted from each request.
#[derive(Clone)]
pub enum TimeoutSource<Req> {
    /// Fixed timeout duration for all requests.
    Fixed(Duration),
    /// Dynamic timeout extracted from the request.
    ///
    /// The function receives a reference to the request and returns
    /// the timeout duration to use for that specific request.
    Dynamic(Arc<dyn Fn(&Req) -> Duration + Send + Sync>),
}

impl<Req> TimeoutSource<Req> {
    /// Get the timeout duration for a request.
    pub fn get_timeout(&self, req: &Req) -> Duration {
        match self {
            TimeoutSource::Fixed(d) => *d,
            TimeoutSource::Dynamic(f) => f(req),
        }
    }
}

impl<Req> Default for TimeoutSource<Req> {
    fn default() -> Self {
        TimeoutSource::Fixed(Duration::from_secs(5))
    }
}

/// Configuration for the time limiter pattern.
pub struct TimeLimiterConfig<Req> {
    pub(crate) timeout_source: TimeoutSource<Req>,
    #[allow(dead_code)]
    pub(crate) cancel_running_future: bool,
    pub(crate) event_listeners: EventListeners<TimeLimiterEvent>,
    pub(crate) name: String,
}

/// Builder for configuring and constructing a time limiter.
pub struct TimeLimiterConfigBuilder<Req> {
    timeout_source: TimeoutSource<Req>,
    cancel_running_future: bool,
    event_listeners: EventListeners<TimeLimiterEvent>,
    name: String,
    _phantom: PhantomData<Req>,
}

impl<Req> TimeLimiterConfigBuilder<Req> {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            timeout_source: TimeoutSource::default(),
            cancel_running_future: false,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
            _phantom: PhantomData,
        }
    }

    /// Sets a fixed timeout duration for all requests.
    ///
    /// This is the simplest configuration where every request gets
    /// the same timeout.
    ///
    /// Default: 5 seconds
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let layer = TimeLimiterLayer::<()>::builder()
    ///     .timeout_duration(Duration::from_secs(30))
    ///     .build();
    /// ```
    pub fn timeout_duration(mut self, duration: Duration) -> Self {
        self.timeout_source = TimeoutSource::Fixed(duration);
        self
    }

    /// Sets a dynamic timeout extractor function.
    ///
    /// The function receives a reference to the request and returns
    /// the timeout duration to use for that specific request. This
    /// enables per-request timeouts based on request properties.
    ///
    /// # Use Cases
    ///
    /// - Extract timeout from HTTP headers (e.g., `X-Timeout-Ms`)
    /// - Honor gRPC deadline propagation
    /// - Different SLAs for different operations
    /// - Priority-based timeout budgets
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// #[derive(Clone)]
    /// struct MyRequest {
    ///     timeout_ms: Option<u64>,
    ///     // ... other fields
    /// }
    ///
    /// let layer = TimeLimiterLayer::<MyRequest>::builder()
    ///     .timeout_fn(|req: &MyRequest| {
    ///         req.timeout_ms
    ///             .map(Duration::from_millis)
    ///             .unwrap_or(Duration::from_secs(5))
    ///     })
    ///     .build();
    /// ```
    ///
    /// # HTTP Header Example
    ///
    /// ```rust,ignore
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let layer = TimeLimiterLayer::<http::Request<Body>>::builder()
    ///     .timeout_fn(|req| {
    ///         req.headers()
    ///             .get("x-timeout-ms")
    ///             .and_then(|v| v.to_str().ok())
    ///             .and_then(|s| s.parse::<u64>().ok())
    ///             .map(Duration::from_millis)
    ///             .unwrap_or(Duration::from_secs(30))
    ///     })
    ///     .build();
    /// ```
    pub fn timeout_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&Req) -> Duration + Send + Sync + 'static,
    {
        self.timeout_source = TimeoutSource::Dynamic(Arc::new(f));
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
    /// before the timeout expires. This is the normal, happy-path case.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the actual duration the call took to complete.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    ///
    /// let config = TimeLimiterLayer::<()>::builder()
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
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let error_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&error_count);
    ///
    /// let config = TimeLimiterLayer::<()>::builder()
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
    /// within the timeout duration. The call will be cancelled (if
    /// `cancel_running_future` is true) or allowed to continue in the background,
    /// and a timeout error will be returned to the caller.
    ///
    /// # Callback Signature
    /// `Fn()` - Called with no parameters when a timeout occurs.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_timelimiter::TimeLimiterLayer;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let timeout_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&timeout_count);
    ///
    /// let config = TimeLimiterLayer::<()>::builder()
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
    pub fn build(self) -> crate::TimeLimiterLayer<Req> {
        let config = TimeLimiterConfig {
            timeout_source: self.timeout_source,
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::TimeLimiterLayer::new(config)
    }
}

impl<Req> Default for TimeLimiterConfigBuilder<Req> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimeLimiterLayer;

    #[test]
    fn test_builder_defaults() {
        let _layer = TimeLimiterLayer::<()>::builder().build();
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = TimeLimiterLayer::<()>::builder()
            .timeout_duration(Duration::from_millis(100))
            .cancel_running_future(true)
            .name("my-timelimiter")
            .build();
    }

    #[test]
    fn test_event_listeners() {
        let _layer = TimeLimiterLayer::<()>::builder()
            .on_success(|_| {})
            .on_error(|_| {})
            .on_timeout(|| {})
            .build();
    }

    #[test]
    fn test_timeout_fn() {
        #[derive(Clone)]
        struct MyRequest {
            timeout_ms: Option<u64>,
        }

        let _layer = TimeLimiterLayer::<MyRequest>::builder()
            .timeout_fn(|req: &MyRequest| {
                req.timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_secs(5))
            })
            .build();
    }

    #[test]
    fn test_timeout_source_fixed() {
        let source: TimeoutSource<()> = TimeoutSource::Fixed(Duration::from_secs(10));
        assert_eq!(source.get_timeout(&()), Duration::from_secs(10));
    }

    #[test]
    fn test_timeout_source_dynamic() {
        #[derive(Clone)]
        struct Req {
            timeout: Duration,
        }

        let source: TimeoutSource<Req> = TimeoutSource::Dynamic(Arc::new(|req: &Req| req.timeout));
        let req = Req {
            timeout: Duration::from_secs(30),
        };
        assert_eq!(source.get_timeout(&req), Duration::from_secs(30));
    }
}
