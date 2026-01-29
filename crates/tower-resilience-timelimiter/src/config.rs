//! Configuration for time limiter.

use crate::events::TimeLimiterEvent;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Trait for determining timeout duration from a request.
///
/// This trait is implemented by both fixed and dynamic timeout sources,
/// enabling type inference for the common case of fixed timeouts.
pub trait TimeoutFn<Req>: Send + Sync {
    /// Get the timeout duration for a request.
    fn get_timeout(&self, req: &Req) -> Duration;

    /// Clone this timeout function into a boxed trait object.
    fn clone_box(&self) -> Box<dyn TimeoutFn<Req>>;
}

/// Fixed timeout that works with any request type.
///
/// This is the default timeout source. Since it ignores the request,
/// it implements `TimeoutFn<Req>` for ALL `Req` types, enabling
/// type inference at the point of use.
#[derive(Debug, Clone, Copy, Default)]
pub struct FixedTimeout(pub(crate) Duration);

impl FixedTimeout {
    /// Create a new fixed timeout with the given duration.
    pub fn new(duration: Duration) -> Self {
        Self(duration)
    }
}

impl<Req> TimeoutFn<Req> for FixedTimeout {
    fn get_timeout(&self, _req: &Req) -> Duration {
        self.0
    }

    fn clone_box(&self) -> Box<dyn TimeoutFn<Req>> {
        Box::new(*self)
    }
}

/// Dynamic timeout extracted from the request.
///
/// This timeout source calls a function with the request to determine
/// the timeout duration, enabling per-request timeouts.
pub struct DynamicTimeout<F> {
    f: Arc<F>,
}

impl<F> Clone for DynamicTimeout<F> {
    fn clone(&self) -> Self {
        Self {
            f: Arc::clone(&self.f),
        }
    }
}

impl<F> DynamicTimeout<F> {
    /// Create a new dynamic timeout with the given function.
    pub fn new(f: F) -> Self {
        Self { f: Arc::new(f) }
    }
}

impl<Req, F> TimeoutFn<Req> for DynamicTimeout<F>
where
    F: Fn(&Req) -> Duration + Send + Sync + 'static,
{
    fn get_timeout(&self, req: &Req) -> Duration {
        (self.f)(req)
    }

    fn clone_box(&self) -> Box<dyn TimeoutFn<Req>> {
        Box::new(self.clone())
    }
}

/// Configuration for the time limiter pattern.
///
/// The type parameter `T` is the timeout source type:
/// - `TimeLimiterConfig<FixedTimeout>` - uses fixed timeout (works with any request type)
/// - `TimeLimiterConfig<DynamicTimeout<F>>` - uses dynamic timeout from request
pub struct TimeLimiterConfig<T> {
    pub(crate) timeout_source: T,
    pub(crate) cancel_running_future: bool,
    pub(crate) event_listeners: EventListeners<TimeLimiterEvent>,
    pub(crate) name: String,
}

impl<T: Clone> Clone for TimeLimiterConfig<T> {
    fn clone(&self) -> Self {
        Self {
            timeout_source: self.timeout_source.clone(),
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners.clone(),
            name: self.name.clone(),
        }
    }
}

/// Builder for configuring and constructing a time limiter.
///
/// The type parameter `T` is the timeout source type. By default, this is
/// `FixedTimeout` which works with any request type. When you call
/// `.timeout_fn()` with a custom function, the type changes to
/// `DynamicTimeout<F>`.
///
/// # Default Usage (no type parameters needed)
///
/// ```rust
/// use tower_resilience_timelimiter::TimeLimiterLayer;
/// use std::time::Duration;
///
/// // No type parameters required for fixed timeout!
/// let layer = TimeLimiterLayer::builder()
///     .timeout_duration(Duration::from_secs(30))
///     .build();
/// ```
///
/// # Dynamic Timeout (types inferred from closure)
///
/// ```rust
/// use tower_resilience_timelimiter::TimeLimiterLayer;
/// use std::time::Duration;
///
/// #[derive(Clone)]
/// struct MyRequest { timeout_ms: Option<u64> }
///
/// let layer = TimeLimiterLayer::builder()
///     .timeout_fn(|req: &MyRequest| {
///         req.timeout_ms
///             .map(Duration::from_millis)
///             .unwrap_or(Duration::from_secs(5))
///     })
///     .build();
/// ```
pub struct TimeLimiterConfigBuilder<T = FixedTimeout> {
    timeout_source: T,
    cancel_running_future: bool,
    event_listeners: EventListeners<TimeLimiterEvent>,
    name: String,
}

impl Default for TimeLimiterConfigBuilder<FixedTimeout> {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeLimiterConfigBuilder<FixedTimeout> {
    /// Creates a new builder with default values.
    ///
    /// The default timeout is 5 seconds with a fixed duration.
    /// No type parameters are required when using the default configuration.
    pub fn new() -> Self {
        Self {
            timeout_source: FixedTimeout(Duration::from_secs(5)),
            cancel_running_future: true,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
        }
    }
}

impl<T> TimeLimiterConfigBuilder<T> {
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
    /// // No type parameters needed!
    /// let layer = TimeLimiterLayer::builder()
    ///     .timeout_duration(Duration::from_secs(30))
    ///     .build();
    /// ```
    pub fn timeout_duration(self, duration: Duration) -> TimeLimiterConfigBuilder<FixedTimeout> {
        TimeLimiterConfigBuilder {
            timeout_source: FixedTimeout(duration),
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners,
            name: self.name,
        }
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
    /// }
    ///
    /// // Types inferred from closure signature
    /// let layer = TimeLimiterLayer::builder()
    ///     .timeout_fn(|req: &MyRequest| {
    ///         req.timeout_ms
    ///             .map(Duration::from_millis)
    ///             .unwrap_or(Duration::from_secs(5))
    ///     })
    ///     .build();
    /// ```
    pub fn timeout_fn<Req, F>(self, f: F) -> TimeLimiterConfigBuilder<DynamicTimeout<F>>
    where
        F: Fn(&Req) -> Duration + Send + Sync + 'static,
    {
        TimeLimiterConfigBuilder {
            timeout_source: DynamicTimeout::new(f),
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners,
            name: self.name,
        }
    }

    /// Sets whether to cancel the running future when a timeout occurs.
    ///
    /// When true (the default), the future will be dropped on timeout, canceling
    /// ongoing work. When false, the future continues running in the background
    /// but its result is ignored.
    ///
    /// Default: true
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
    pub fn build(self) -> crate::TimeLimiterLayer<T> {
        let config = TimeLimiterConfig {
            timeout_source: self.timeout_source,
            cancel_running_future: self.cancel_running_future,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::TimeLimiterLayer::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimeLimiterLayer;

    #[test]
    fn test_builder_defaults() {
        // No type parameters needed!
        let _layer = TimeLimiterLayer::builder().build();
    }

    #[test]
    fn test_builder_custom_values() {
        // No type parameters needed for fixed timeout!
        let _layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(100))
            .cancel_running_future(true)
            .name("my-timelimiter")
            .build();
    }

    #[test]
    fn test_event_listeners() {
        let _layer = TimeLimiterLayer::builder()
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

        // Types inferred from closure
        let _layer = TimeLimiterLayer::builder()
            .timeout_fn(|req: &MyRequest| {
                req.timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_secs(5))
            })
            .build();
    }

    #[test]
    fn test_fixed_timeout() {
        let timeout = FixedTimeout::new(Duration::from_secs(10));
        assert_eq!(timeout.get_timeout(&()), Duration::from_secs(10));
        assert_eq!(timeout.get_timeout(&"any type"), Duration::from_secs(10));
    }

    #[test]
    fn test_dynamic_timeout() {
        #[derive(Clone)]
        struct Req {
            timeout: Duration,
        }

        let timeout = DynamicTimeout::new(|req: &Req| req.timeout);
        let req = Req {
            timeout: Duration::from_secs(30),
        };
        assert_eq!(timeout.get_timeout(&req), Duration::from_secs(30));
    }
}
