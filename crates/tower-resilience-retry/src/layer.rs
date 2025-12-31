use crate::{Retry, RetryConfig};
use std::marker::PhantomData;
use std::sync::Arc;
use tower::Layer;

/// A Tower [`Layer`] that applies retry logic to a service.
///
/// This layer wraps a service with retry middleware that automatically
/// retries failed requests according to the configured policy.
///
/// # Examples
///
/// ## Fixed max attempts (simple)
///
/// ```
/// use tower_resilience_retry::RetryLayer;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # #[derive(Debug, Clone)]
/// # struct MyError;
/// # async fn example() {
/// let retry_layer = RetryLayer::<String, MyError>::builder()
///     .max_attempts(5)
///     .exponential_backoff(Duration::from_millis(100))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(retry_layer)
///     .service(my_service());
/// # }
/// # fn my_service() -> impl tower::Service<String, Response = String, Error = MyError> {
/// #     tower::service_fn(|req: String| async move { Ok::<_, MyError>(req) })
/// # }
/// ```
///
/// ## Per-request max attempts (dynamic)
///
/// ```
/// use tower_resilience_retry::RetryLayer;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// #[derive(Clone)]
/// struct MyRequest {
///     is_idempotent: bool,
///     data: String,
/// }
///
/// # #[derive(Debug, Clone)]
/// # struct MyError;
/// # async fn example() {
/// // Idempotent requests can retry more, non-idempotent get 1 attempt
/// let retry_layer = RetryLayer::<MyRequest, MyError>::builder()
///     .max_attempts_fn(|req: &MyRequest| {
///         if req.is_idempotent { 5 } else { 1 }
///     })
///     .exponential_backoff(Duration::from_millis(100))
///     .build();
/// # }
/// ```
#[derive(Clone)]
pub struct RetryLayer<Req, E> {
    config: Arc<RetryConfig<Req, E>>,
}

impl<Req, E> RetryLayer<Req, E> {
    /// Creates a new `RetryLayer` with the given configuration.
    pub fn new(config: RetryConfig<Req, E>) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Creates a new builder for configuring a retry layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    ///
    /// # #[derive(Debug, Clone)]
    /// # struct MyError;
    /// let layer = RetryLayer::<(), MyError>::builder()
    ///     .max_attempts(5)
    ///     .exponential_backoff(Duration::from_millis(100))
    ///     .build();
    /// ```
    pub fn builder() -> crate::RetryConfigBuilder<Req, E> {
        crate::RetryConfigBuilder::new()
    }
}

impl<S, Req, E> Layer<S> for RetryLayer<Req, E>
where
    E: Clone,
    Req: 'static,
{
    type Service = Retry<S, Req, E>;

    fn layer(&self, service: S) -> Self::Service {
        Retry::new(service, Arc::clone(&self.config), PhantomData)
    }
}
