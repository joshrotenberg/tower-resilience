//! Configuration for the fallback service.

use crate::{FallbackEvent, FallbackStrategy, HandlePredicate};
use tower_resilience_core::{EventListeners, FnListener};

/// Configuration for the fallback service.
pub struct FallbackConfig<Req, Res, E> {
    pub(crate) name: String,
    pub(crate) strategy: FallbackStrategy<Req, Res, E>,
    pub(crate) handle_predicate: Option<HandlePredicate<E>>,
    pub(crate) event_listeners: EventListeners<FallbackEvent>,
}

/// Builder for constructing a [`FallbackLayer`](crate::FallbackLayer).
pub struct FallbackConfigBuilder<Req, Res, E> {
    name: String,
    strategy: Option<FallbackStrategy<Req, Res, E>>,
    handle_predicate: Option<HandlePredicate<E>>,
    event_listeners: EventListeners<FallbackEvent>,
}

impl<Req, Res, E> Default for FallbackConfigBuilder<Req, Res, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Req, Res, E> FallbackConfigBuilder<Req, Res, E> {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self {
            name: "fallback".to_string(),
            strategy: None,
            handle_predicate: None,
            event_listeners: EventListeners::new(),
        }
    }

    /// Sets the name for this fallback instance (used in metrics and events).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets a static fallback value.
    ///
    /// Note: This requires `Res: Clone`. If your response type doesn't implement
    /// Clone, use [`value_fn`](Self::value_fn) instead.
    pub fn value(mut self, value: Res) -> Self
    where
        Res: Clone,
    {
        self.strategy = Some(FallbackStrategy::Value(value));
        self
    }

    /// Sets a fallback value generator function.
    ///
    /// Unlike [`value`](Self::value), this doesn't require `Res: Clone` since
    /// the function generates a fresh value for each fallback.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug)]
    /// # struct MyError;
    /// # struct MyResponse { data: Vec<u8> }  // No Clone!
    /// let layer: FallbackLayer<String, MyResponse, MyError> = FallbackLayer::builder()
    ///     .value_fn(|| MyResponse { data: vec![0; 1024] })
    ///     .build();
    /// ```
    pub fn value_fn<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Res + Send + Sync + 'static,
    {
        self.strategy = Some(FallbackStrategy::ValueFn(std::sync::Arc::new(f)));
        self
    }

    /// Sets a fallback function that computes a response from the error.
    pub fn from_error<F>(mut self, f: F) -> Self
    where
        F: Fn(&E) -> Res + Send + Sync + 'static,
    {
        self.strategy = Some(FallbackStrategy::FromError(std::sync::Arc::new(f)));
        self
    }

    /// Sets a fallback function that has access to both request and error.
    pub fn from_request_error<F>(mut self, f: F) -> Self
    where
        F: Fn(&Req, &E) -> Res + Send + Sync + 'static,
    {
        self.strategy = Some(FallbackStrategy::FromRequestError(std::sync::Arc::new(f)));
        self
    }

    /// Sets a backup service to call on failure.
    pub fn service<S, Fut>(mut self, service: S) -> Self
    where
        S: Fn(Req) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Res, E>> + Send + 'static,
    {
        self.strategy = Some(FallbackStrategy::Service(std::sync::Arc::new(move |req| {
            Box::pin(service(req))
        })));
        self
    }

    /// Sets an error transformation function.
    pub fn exception<F>(mut self, f: F) -> Self
    where
        F: Fn(E) -> E + Send + Sync + 'static,
    {
        self.strategy = Some(FallbackStrategy::Exception(std::sync::Arc::new(f)));
        self
    }

    /// Only trigger fallback for errors matching this predicate.
    ///
    /// Errors that don't match the predicate will be propagated as-is.
    pub fn handle<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&E) -> bool + Send + Sync + 'static,
    {
        self.handle_predicate = Some(std::sync::Arc::new(predicate));
        self
    }

    /// Adds an event listener.
    pub fn on_event<F>(mut self, listener: F) -> Self
    where
        F: Fn(&FallbackEvent) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(listener));
        self
    }

    /// Builds the fallback layer.
    ///
    /// # Panics
    ///
    /// Panics if no fallback strategy was configured.
    pub fn build(self) -> crate::FallbackLayer<Req, Res, E> {
        let config = FallbackConfig {
            name: self.name,
            strategy: self.strategy.expect("fallback strategy must be set"),
            handle_predicate: self.handle_predicate,
            event_listeners: self.event_listeners,
        };
        crate::FallbackLayer::new(config)
    }
}
