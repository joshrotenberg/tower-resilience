//! Tower layer for fallback.

use crate::config::{FallbackConfig, FallbackConfigBuilder};
use crate::Fallback;
use std::sync::Arc;
use tower::layer::Layer;

/// A Tower layer that applies fallback behavior to a service.
///
/// See the [module-level documentation](crate) for usage examples.
pub struct FallbackLayer<Req, Res, E> {
    config: Arc<FallbackConfig<Req, Res, E>>,
}

impl<Req, Res, E> FallbackLayer<Req, Res, E> {
    /// Creates a new fallback layer from the given configuration.
    pub(crate) fn new(config: FallbackConfig<Req, Res, E>) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Creates a new builder for configuring a fallback layer.
    pub fn builder() -> FallbackConfigBuilder<Req, Res, E> {
        FallbackConfigBuilder::new()
    }

    /// Creates a fallback layer that generates a value using a function.
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
    /// # struct MyResponse { data: Vec<u8> }  // No Clone needed!
    /// let layer = FallbackLayer::<String, MyResponse, MyError>::value_fn(|| {
    ///     MyResponse { data: vec![0; 1024] }
    /// });
    /// ```
    pub fn value_fn<F>(f: F) -> Self
    where
        F: Fn() -> Res + Send + Sync + 'static,
    {
        FallbackConfigBuilder::new().value_fn(f).build()
    }
}

// Convenience constructors for common patterns
impl<Req, Res, E> FallbackLayer<Req, Res, E>
where
    Res: Clone,
{
    /// Creates a fallback layer that returns a static value on failure.
    ///
    /// Note: This requires `Res: Clone`. If your response type doesn't implement
    /// Clone, use [`value_fn`](Self::value_fn) instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError;
    /// let layer = FallbackLayer::<String, String, MyError>::value("default".to_string());
    /// ```
    pub fn value(value: Res) -> Self {
        FallbackConfigBuilder::new().value(value).build()
    }

    /// Creates a fallback layer that computes a response from the error.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError { msg: String }
    /// let layer = FallbackLayer::<String, String, MyError>::from_error(|e| {
    ///     format!("Error: {}", e.msg)
    /// });
    /// ```
    pub fn from_error<F>(f: F) -> Self
    where
        F: Fn(&E) -> Res + Send + Sync + 'static,
    {
        FallbackConfigBuilder::new().from_error(f).build()
    }

    /// Creates a fallback layer that computes a response from request and error.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError;
    /// let layer = FallbackLayer::<String, String, MyError>::from_request_error(|req, _err| {
    ///     format!("Fallback for request: {}", req)
    /// });
    /// ```
    pub fn from_request_error<F>(f: F) -> Self
    where
        F: Fn(&Req, &E) -> Res + Send + Sync + 'static,
    {
        FallbackConfigBuilder::new().from_request_error(f).build()
    }

    /// Creates a fallback layer that routes to a backup service.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError;
    /// let layer = FallbackLayer::<String, String, MyError>::service(|req: String| async move {
    ///     Ok::<_, MyError>(format!("backup: {}", req))
    /// });
    /// ```
    pub fn service<S, Fut>(service: S) -> Self
    where
        S: Fn(Req) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Res, E>> + Send + 'static,
    {
        FallbackConfigBuilder::new().service(service).build()
    }

    /// Creates a fallback layer that transforms errors.
    ///
    /// Note: This still returns an error, just a transformed one.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_fallback::FallbackLayer;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError { code: u32 }
    /// let layer = FallbackLayer::<String, String, MyError>::exception(|e| {
    ///     MyError { code: 500 }
    /// });
    /// ```
    pub fn exception<F>(f: F) -> Self
    where
        F: Fn(E) -> E + Send + Sync + 'static,
    {
        FallbackConfigBuilder::new().exception(f).build()
    }
}

impl<Req, Res, E> Clone for FallbackLayer<Req, Res, E>
where
    Res: Clone,
{
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
        }
    }
}

impl<S, Req, Res, E> Layer<S> for FallbackLayer<Req, Res, E>
where
    Res: Clone,
{
    type Service = Fallback<S, Req, Res, E>;

    fn layer(&self, service: S) -> Self::Service {
        Fallback::new(service, Arc::clone(&self.config))
    }
}
