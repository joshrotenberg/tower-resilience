//! Unified error layer for composing multiple resilience patterns.
//!
//! When stacking multiple resilience layers, each layer produces its own error type
//! (e.g., `BulkheadServiceError<E>`, `CircuitBreakerError<E>`). Without unification,
//! these nest: `CircuitBreakerError<BulkheadServiceError<E>>`, forcing per-layer
//! error wrapping.
//!
//! [`ResilienceErrorLayer`] wraps any resilience layer so its service produces
//! [`ResilienceError<E>`](crate::ResilienceError) instead, where `E` is the
//! application error type.
//!
//! # Example
//!
//! ```rust,ignore
//! use tower_resilience_core::ResilienceErrorLayer;
//!
//! // Single layer: error type is ResilienceError<AppError>
//! let svc = ServiceBuilder::new()
//!     .layer(ResilienceErrorLayer::<_, AppError>::new(BulkheadLayer::medium().build()))
//!     .service(my_service);
//! ```

use crate::error::IntoResilienceError;
use crate::ResilienceError;
use pin_project_lite::pin_project;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// A Tower Layer that wraps another layer to unify its error type to [`ResilienceError<E>`].
///
/// The type parameter `E` is the application error type.
#[derive(Debug)]
pub struct ResilienceErrorLayer<L, E> {
    inner: L,
    _marker: PhantomData<fn() -> E>,
}

impl<L: Clone, E> Clone for ResilienceErrorLayer<L, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl<L, E> ResilienceErrorLayer<L, E> {
    /// Wraps a layer so its service's errors are converted to `ResilienceError<E>`.
    pub fn new(inner: L) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

impl<L, S, E> Layer<S> for ResilienceErrorLayer<L, E>
where
    L: Layer<S>,
{
    type Service = ResilienceErrorService<L::Service, E>;

    fn layer(&self, service: S) -> Self::Service {
        ResilienceErrorService::new(self.inner.layer(service))
    }
}

/// A Tower Service that maps the inner service's errors to [`ResilienceError<E>`].
///
/// Created by [`ResilienceErrorLayer`]. You typically don't construct this directly.
#[derive(Debug)]
pub struct ResilienceErrorService<S, E> {
    inner: S,
    _marker: PhantomData<fn() -> E>,
}

impl<S: Clone, E> Clone for ResilienceErrorService<S, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl<S, E> ResilienceErrorService<S, E> {
    /// Wraps a service so its errors are converted to `ResilienceError<E>`.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

impl<S, Request, E> Service<Request> for ResilienceErrorService<S, E>
where
    S: Service<Request>,
    S::Error: IntoResilienceError<E>,
{
    type Response = S::Response;
    type Error = ResilienceError<E>;
    type Future = ResilienceErrorFuture<S::Future, E>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(IntoResilienceError::into_resilience_error)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        ResilienceErrorFuture {
            inner: self.inner.call(request),
            _marker: PhantomData,
        }
    }
}

pin_project! {
    /// Future returned by [`ResilienceErrorService`].
    pub struct ResilienceErrorFuture<F, E> {
        #[pin]
        inner: F,
        _marker: PhantomData<fn() -> E>,
    }
}

impl<F, T, InnerErr, E> std::future::Future for ResilienceErrorFuture<F, E>
where
    F: std::future::Future<Output = Result<T, InnerErr>>,
    InnerErr: IntoResilienceError<E>,
{
    type Output = Result<T, ResilienceError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.inner
            .poll(cx)
            .map_err(IntoResilienceError::into_resilience_error)
    }
}

/// Extension trait providing `.unified()` on any layer.
///
/// This is syntactic sugar for [`ResilienceErrorLayer::new(layer)`](ResilienceErrorLayer::new).
///
/// # Examples
///
/// ```rust,ignore
/// use tower_resilience_core::UnifiedErrors;
///
/// let layer = BulkheadLayer::medium().build().unified::<AppError>();
/// ```
pub trait UnifiedErrors: Sized {
    /// Wraps this layer so its service errors are converted to `ResilienceError<E>`.
    ///
    /// The type parameter `E` is the application error type. It can usually be
    /// inferred from context, but may need to be specified explicitly.
    fn unified<E>(self) -> ResilienceErrorLayer<Self, E> {
        ResilienceErrorLayer::new(self)
    }
}

impl<L> UnifiedErrors for L {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use tower::layer::util::Identity;
    use tower::ServiceExt;

    #[derive(Debug, Clone)]
    struct TestAppError(String);

    impl fmt::Display for TestAppError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for TestAppError {}

    #[tokio::test]
    async fn test_passthrough_on_success() {
        // Identity layer doesn't change the error type, so for this to work
        // the service error must already satisfy IntoResilienceError.
        // BulkheadServiceError<AppError> -> ResilienceError<AppError> via blanket impl.
        // Here we test with ResilienceError<E> directly (identity passthrough).
        let svc = tower::service_fn(|req: String| async move {
            Ok::<_, ResilienceError<TestAppError>>(req.to_uppercase())
        });

        let layer = ResilienceErrorLayer::<_, TestAppError>::new(Identity::new());
        let mut svc = layer.layer(svc);

        let resp: Result<String, _> = svc.ready().await.unwrap().call("hello".into()).await;
        assert_eq!(resp.unwrap(), "HELLO");
    }

    #[tokio::test]
    async fn test_resilience_error_passes_through() {
        let svc = tower::service_fn(|_req: String| async {
            Err::<String, ResilienceError<TestAppError>>(ResilienceError::CircuitOpen {
                name: Some("test".into()),
            })
        });

        let layer = ResilienceErrorLayer::<_, TestAppError>::new(Identity::new());
        let mut svc = layer.layer(svc);

        let err: ResilienceError<TestAppError> = svc
            .ready()
            .await
            .unwrap()
            .call("hello".into())
            .await
            .unwrap_err();
        assert!(err.is_circuit_open());
    }

    #[tokio::test]
    async fn test_unified_extension_trait() {
        let svc =
            tower::service_fn(
                |req: String| async move { Ok::<_, ResilienceError<TestAppError>>(req) },
            );

        let layer = Identity::new().unified::<TestAppError>();
        let mut svc = layer.layer(svc);

        let resp: Result<String, _> = svc.ready().await.unwrap().call("test".into()).await;
        assert!(resp.is_ok());
    }
}
