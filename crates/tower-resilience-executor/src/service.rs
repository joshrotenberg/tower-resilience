//! Service implementation for the executor middleware.

use crate::Executor;
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::oneshot;
use tower_service::Service;

/// A service that delegates request processing to an executor.
///
/// Each request is spawned as a new task on the executor, allowing
/// parallel processing of multiple requests.
///
/// # Requirements
///
/// The inner service must implement `Clone` so that each spawned task
/// can have its own instance. This is the standard pattern for Tower
/// services that need to be shared across tasks.
///
/// # Cancellation
///
/// When the response future is dropped, the spawned task continues
/// to run to completion. This is intentional to avoid partial processing.
/// If you need cancellation, consider wrapping with a timeout layer.
#[derive(Clone)]
pub struct ExecutorService<S, E> {
    inner: S,
    executor: E,
}

impl<S, E> ExecutorService<S, E> {
    /// Creates a new executor service.
    pub fn new(service: S, executor: E) -> Self {
        Self {
            inner: service,
            executor,
        }
    }

    /// Returns a reference to the inner service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Returns a mutable reference to the inner service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consumes the service and returns the inner service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, E, Req> Service<Req> for ExecutorService<S, E>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Future: Send,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    E: Executor,
    Req: Send + 'static,
{
    type Response = S::Response;
    type Error = ExecutorError<S::Error>;
    type Future = ExecutorFuture<S::Response, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Poll the inner service for readiness
        self.inner.poll_ready(cx).map_err(ExecutorError::Service)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        // Clone the service for the spawned task
        let mut service = self.inner.clone();
        let (tx, rx) = oneshot::channel();

        // Spawn the request processing on the executor
        let _handle = self.executor.spawn(async move {
            // Call the service
            let result = service.call(req).await;

            // Send the result back
            // The send may fail if the receiver is dropped (caller cancelled)
            // We ignore this error since there's nothing useful to do.
            let _ = tx.send(result.map_err(ExecutorError::Service));
        });

        ExecutorFuture { rx }
    }
}

/// Error type for executor service operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorError<E> {
    /// The spawned task was cancelled or panicked.
    TaskCancelled,
    /// The inner service returned an error.
    Service(E),
}

impl<E: std::fmt::Display> std::fmt::Display for ExecutorError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TaskCancelled => write!(f, "executor task was cancelled"),
            Self::Service(e) => write!(f, "service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ExecutorError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Service(e) => Some(e),
            _ => None,
        }
    }
}

pin_project! {
    /// Future returned by [`ExecutorService`].
    pub struct ExecutorFuture<T, E> {
        #[pin]
        rx: oneshot::Receiver<Result<T, ExecutorError<E>>>,
    }
}

impl<T, E> Future for ExecutorFuture<T, E> {
    type Output = Result<T, ExecutorError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.rx.poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(ExecutorError::TaskCancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err: ExecutorError<std::io::Error> = ExecutorError::TaskCancelled;
        assert_eq!(err.to_string(), "executor task was cancelled");
    }

    #[test]
    fn test_error_eq() {
        let err1: ExecutorError<&str> = ExecutorError::TaskCancelled;
        let err2: ExecutorError<&str> = ExecutorError::TaskCancelled;
        assert_eq!(err1, err2);

        let err3: ExecutorError<&str> = ExecutorError::Service("test");
        let err4: ExecutorError<&str> = ExecutorError::Service("test");
        assert_eq!(err3, err4);
    }
}
