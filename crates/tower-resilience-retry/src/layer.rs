use crate::{Retry, RetryConfig};
use std::sync::Arc;
use tower::Layer;

/// A Tower [`Layer`] that applies retry logic to a service.
///
/// This layer wraps a service with retry middleware that automatically
/// retries failed requests according to the configured policy.
///
/// # Examples
///
/// ```
/// use tower_resilience_retry::RetryConfig;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # #[derive(Debug, Clone)]
/// # struct MyError;
/// # async fn example() {
/// let retry_layer: tower_resilience_retry::RetryLayer<MyError> = RetryConfig::builder()
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
#[derive(Clone)]
pub struct RetryLayer<E> {
    config: Arc<RetryConfig<E>>,
}

impl<E> RetryLayer<E> {
    /// Creates a new `RetryLayer` with the given configuration.
    pub fn new(config: RetryConfig<E>) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl<S, E> Layer<S> for RetryLayer<E>
where
    E: Clone,
{
    type Service = Retry<S, E>;

    fn layer(&self, service: S) -> Self::Service {
        Retry::new(service, Arc::clone(&self.config))
    }
}
