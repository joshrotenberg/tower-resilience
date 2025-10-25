use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use pin_project::pin_project;
use tower::Service;

use crate::{config::ReconnectConfig, state::ReconnectState};

/// A Tower Service that automatically reconnects on connection failures.
///
/// This is a placeholder implementation that will be enhanced to support
/// automatic reconnection with configurable backoff strategies.
///
/// # Type Parameters
///
/// * `S` - The inner service
#[derive(Clone)]
pub struct ReconnectService<S> {
    inner: S,
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
}

impl<S> ReconnectService<S> {
    /// Creates a new `ReconnectService` wrapping the given service.
    pub(crate) fn new(inner: S, config: Arc<ReconnectConfig>, state: ReconnectState) -> Self {
        Self {
            inner,
            config,
            state,
        }
    }

    /// Returns a reference to the current reconnection state.
    pub fn state(&self) -> &ReconnectState {
        &self.state
    }

    /// Returns a reference to the reconnection configuration.
    pub fn config(&self) -> &ReconnectConfig {
        &self.config
    }
}

impl<S, Request> Service<Request> for ReconnectService<S>
where
    S: Service<Request> + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
    Request: Clone,
{
    type Response = S::Response;
    type Error = ReconnectError<S::Error>;
    type Future = ReconnectFuture<S, Request>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ReconnectError::ServiceError)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let call_future = self.inner.call(request.clone());
        ReconnectFuture {
            inner: self.inner.clone(),
            config: self.config.clone(),
            state: self.state.clone(),
            request,
            attempt: 0,
            phase: Phase::Calling(call_future),
        }
    }
}

/// Future returned by `ReconnectService`.
#[pin_project]
pub struct ReconnectFuture<S, Request>
where
    S: Service<Request>,
{
    inner: S,
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
    request: Request,
    attempt: u32,
    #[pin]
    phase: Phase<S::Future>,
}

#[pin_project(project = PhaseProj)]
enum Phase<F> {
    Calling(#[pin] F),
    Sleeping(#[pin] tokio::time::Sleep),
    Failed,
}

impl<S, Request> Future for ReconnectFuture<S, Request>
where
    S: Service<Request>,
    S::Error: std::error::Error + Send + Sync + 'static,
    Request: Clone,
{
    type Output = Result<S::Response, ReconnectError<S::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.phase.as_mut().project() {
                PhaseProj::Calling(call_future) => {
                    match call_future.poll(cx) {
                        Poll::Ready(Ok(response)) => {
                            this.state.mark_connected();
                            return Poll::Ready(Ok(response));
                        }
                        Poll::Ready(Err(error)) => {
                            this.state.mark_disconnected();
                            *this.attempt += 1;

                            // Check if we've exceeded max attempts
                            if let Some(max) = this.config.max_attempts {
                                if *this.attempt > max {
                                    this.phase.set(Phase::Failed);
                                    return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                                        attempts: *this.attempt,
                                        error: Box::new(error),
                                    }));
                                }
                            }

                            // Get delay for this attempt
                            if let Some(delay) =
                                this.config.policy.delay_for_attempt(*this.attempt as usize)
                            {
                                this.state.mark_reconnecting();

                                #[cfg(feature = "tracing")]
                                if let Some(ref callback) = this.config.on_reconnect {
                                    callback(*this.attempt);
                                }

                                this.phase.set(Phase::Sleeping(tokio::time::sleep(delay)));
                            } else {
                                // No backoff - fail immediately
                                this.phase.set(Phase::Failed);
                                return Poll::Ready(Err(ReconnectError::ConnectionFailed(error)));
                            }
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                PhaseProj::Sleeping(sleep) => {
                    match sleep.poll(cx) {
                        Poll::Ready(()) => {
                            // Sleep complete, try calling again
                            let call_future = this.inner.call(this.request.clone());
                            this.phase.set(Phase::Calling(call_future));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                PhaseProj::Failed => {
                    panic!("ReconnectFuture polled after completion");
                }
            }
        }
    }
}

/// Errors that can occur during reconnection.
#[derive(Debug)]
pub enum ReconnectError<E> {
    /// The maximum number of reconnection attempts was exceeded.
    MaxAttemptsExceeded {
        /// The number of attempts made.
        attempts: u32,
        /// The last error encountered.
        error: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to establish a connection.
    ConnectionFailed(E),

    /// The service returned an error.
    ServiceError(E),
}

impl<E> std::fmt::Display for ReconnectError<E>
where
    E: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxAttemptsExceeded { attempts, error } => {
                write!(
                    f,
                    "max reconnection attempts ({}) exceeded: {}",
                    attempts, error
                )
            }
            Self::ConnectionFailed(e) => write!(f, "connection failed: {}", e),
            Self::ServiceError(e) => write!(f, "service error: {}", e),
        }
    }
}

impl<E> std::error::Error for ReconnectError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MaxAttemptsExceeded { error, .. } => Some(error.as_ref()),
            Self::ConnectionFailed(e) => Some(e),
            Self::ServiceError(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReconnectConfig, ReconnectPolicy};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[derive(Clone)]
    struct FailingService {
        fail_count: Arc<AtomicUsize>,
        max_fails: usize,
    }

    impl Service<String> for FailingService {
        type Response = String;
        type Error = std::io::Error;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: String) -> Self::Future {
            let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
            let max_fails = self.max_fails;

            Box::pin(async move {
                if count < max_fails {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        "mock connection failure",
                    ))
                } else {
                    Ok(format!("echo: {}", req))
                }
            })
        }
    }

    #[tokio::test]
    async fn test_service_successful_on_first_try() {
        let inner = FailingService {
            fail_count: Arc::new(AtomicUsize::new(0)),
            max_fails: 0, // Don't fail
        };

        let config = ReconnectConfig::default();
        let state = ReconnectState::new();
        let mut service = ReconnectService::new(inner, Arc::new(config), state);

        let response = service.call("test".to_string()).await.unwrap();
        assert_eq!(response, "echo: test");
    }

    #[tokio::test]
    async fn test_service_reconnects_after_failure() {
        let inner = FailingService {
            fail_count: Arc::new(AtomicUsize::new(0)),
            max_fails: 2, // Fail twice, then succeed
        };

        let config = ReconnectConfig::builder()
            .policy(ReconnectPolicy::exponential(
                Duration::from_millis(10),
                Duration::from_millis(100),
            ))
            .max_attempts(5)
            .build();

        let state = ReconnectState::new();
        let mut service = ReconnectService::new(inner, Arc::new(config), state.clone());

        let response = service.call("test".to_string()).await.unwrap();
        assert_eq!(response, "echo: test");
        // After successful connection, attempts are reset to 0
        assert_eq!(state.attempts(), 0);
    }

    #[tokio::test]
    async fn test_service_fails_after_max_attempts() {
        let inner = FailingService {
            fail_count: Arc::new(AtomicUsize::new(0)),
            max_fails: 100, // Always fail
        };

        let config = ReconnectConfig::builder()
            .policy(ReconnectPolicy::exponential(
                Duration::from_millis(10),
                Duration::from_millis(50),
            ))
            .max_attempts(3)
            .build();

        let state = ReconnectState::new();
        let mut service = ReconnectService::new(inner, Arc::new(config), state.clone());

        let result = service.call("test".to_string()).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ReconnectError::MaxAttemptsExceeded { attempts, .. } => {
                assert_eq!(attempts, 4); // Initial + 3 retries
            }
            _ => panic!("Expected MaxAttemptsExceeded error"),
        }
    }
}
