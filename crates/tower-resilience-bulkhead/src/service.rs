//! Bulkhead service implementation.

use crate::config::BulkheadConfig;
use crate::error::{BulkheadError, BulkheadServiceError};
use crate::events::BulkheadEvent;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::PollSemaphore;
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, gauge, histogram};

/// Bulkhead service that limits concurrent calls.
pub struct Bulkhead<S> {
    inner: S,
    semaphore: Arc<Semaphore>,
    config: Arc<BulkheadConfig>,
    /// Directly pollable semaphore acquisition state local to this clone.
    poll_semaphore: PollSemaphore,
    /// Permit reserved in `poll_ready` (backpressure mode only).
    permit: Option<OwnedSemaphorePermit>,
}

impl<S: Clone> Clone for Bulkhead<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            semaphore: Arc::clone(&self.semaphore),
            config: Arc::clone(&self.config),
            poll_semaphore: PollSemaphore::new(Arc::clone(&self.semaphore)),
            permit: None,
        }
    }
}

impl<S> Bulkhead<S> {
    /// Creates a new bulkhead service.
    pub(crate) fn new(inner: S, config: BulkheadConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_calls));
        Self {
            inner,
            semaphore: Arc::clone(&semaphore),
            config: Arc::new(config),
            poll_semaphore: PollSemaphore::new(semaphore),
            permit: None,
        }
    }

    /// Creates a new bulkhead service using pre-created shared state.
    pub(crate) fn from_shared(
        inner: S,
        semaphore: Arc<Semaphore>,
        config: Arc<BulkheadConfig>,
    ) -> Self {
        Self {
            inner,
            semaphore: Arc::clone(&semaphore),
            config,
            poll_semaphore: PollSemaphore::new(semaphore),
            permit: None,
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

    /// Consumes the bulkhead, returning the inner service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, Request> Service<Request> for Bulkhead<S>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = BulkheadServiceError<S::Error>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if !self.config.backpressure {
            return self
                .inner
                .poll_ready(cx)
                .map_err(BulkheadServiceError::Inner);
        }

        // Backpressure mode: reserve capacity before reporting readiness. The
        // pollable semaphore keeps its own FIFO waiter and is cancelled simply
        // by dropping this service clone; no detached task is involved.
        if self.permit.is_none() {
            match self.poll_semaphore.poll_acquire(cx) {
                Poll::Ready(Some(permit)) => self.permit = Some(permit),
                Poll::Ready(None) => return Poll::Ready(Err(BulkheadError::Closed.into())),
                Poll::Pending => return Poll::Pending,
            }
        }

        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.permit = None;
                Poll::Ready(Err(BulkheadServiceError::Inner(error)))
            }
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if self.config.backpressure {
            let Some(permit) = self.permit.take() else {
                return Box::pin(async { Err(BulkheadError::NotReady.into()) });
            };
            // Backpressure mode: permit already acquired in poll_ready
            let semaphore_for_check = Arc::clone(&self.semaphore);
            let config = Arc::clone(&self.config);
            let clone = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, clone);
            let start_time = Instant::now();

            // Emit call permitted event
            let concurrent_calls =
                config.max_concurrent_calls - semaphore_for_check.available_permits();
            let event = BulkheadEvent::CallPermitted {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                concurrent_calls,
            };
            config.event_listeners.emit(&event);

            #[cfg(feature = "metrics")]
            {
                counter!("bulkhead_calls_permitted_total", "bulkhead" => config.name.clone())
                    .increment(1);
                gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                    .set(concurrent_calls as f64);
                histogram!("bulkhead_wait_duration_seconds", "bulkhead" => config.name.clone())
                    .record(0.0);
            }

            return Box::pin(async move {
                let result = inner.call(request).await;
                drop(permit);
                let duration = start_time.elapsed();

                match &result {
                    Ok(_) => {
                        let event = BulkheadEvent::CallFinished {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            duration,
                        };
                        config.event_listeners.emit(&event);

                        #[cfg(feature = "metrics")]
                        {
                            counter!("bulkhead_calls_finished_total", "bulkhead" => config.name.clone())
                                .increment(1);
                            histogram!("bulkhead_call_duration_seconds", "bulkhead" => config.name.clone())
                                .record(duration.as_secs_f64());
                        }
                    }
                    Err(_) => {
                        let event = BulkheadEvent::CallFailed {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            duration,
                        };
                        config.event_listeners.emit(&event);

                        #[cfg(feature = "metrics")]
                        {
                            counter!("bulkhead_calls_failed_total", "bulkhead" => config.name.clone())
                                .increment(1);
                            histogram!("bulkhead_call_duration_seconds", "bulkhead" => config.name.clone())
                                .record(duration.as_secs_f64());
                        }
                    }
                }

                #[cfg(feature = "metrics")]
                {
                    let new_concurrent =
                        config.max_concurrent_calls - semaphore_for_check.available_permits();
                    gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                        .set(new_concurrent as f64);
                }

                result.map_err(BulkheadServiceError::Inner)
            });
        }

        // Rejection mode: acquire permit in call
        let semaphore = Arc::clone(&self.semaphore);
        let semaphore_for_check = Arc::clone(&self.semaphore);
        let config = Arc::clone(&self.config);
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let start_time = Instant::now();

        #[cfg(feature = "metrics")]
        let acquire_start = Instant::now();

        Box::pin(async move {
            // Try to acquire a permit
            let permit = match config.max_wait_duration {
                Some(duration) => {
                    match tokio::time::timeout(duration, semaphore.acquire_owned()).await {
                        Ok(Ok(permit)) => permit,
                        Ok(Err(_)) => {
                            // Semaphore was closed while the caller was waiting.
                            let event = BulkheadEvent::CallRejected {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                max_concurrent_calls: config.max_concurrent_calls,
                            };
                            config.event_listeners.emit(&event);

                            #[cfg(feature = "metrics")]
                            counter!("bulkhead_calls_rejected_total", "bulkhead" => config.name.clone())
                                .increment(1);

                            return Err(BulkheadError::Closed.into());
                        }
                        Err(_) => {
                            // Timeout
                            let event = BulkheadEvent::CallRejected {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                max_concurrent_calls: config.max_concurrent_calls,
                            };
                            config.event_listeners.emit(&event);

                            #[cfg(feature = "metrics")]
                            counter!("bulkhead_calls_rejected_total", "bulkhead" => config.name.clone())
                                .increment(1);

                            return Err(BulkheadError::Timeout.into());
                        }
                    }
                }
                None => {
                    // Wait indefinitely
                    match semaphore.acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => {
                            // Semaphore was closed
                            let event = BulkheadEvent::CallRejected {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                max_concurrent_calls: config.max_concurrent_calls,
                            };
                            config.event_listeners.emit(&event);

                            #[cfg(feature = "metrics")]
                            counter!("bulkhead_calls_rejected_total", "bulkhead" => config.name.clone())
                                .increment(1);

                            return Err(BulkheadError::Closed.into());
                        }
                    }
                }
            };

            // Emit call permitted event
            let concurrent_calls =
                config.max_concurrent_calls - semaphore_for_check.available_permits();
            let event = BulkheadEvent::CallPermitted {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                concurrent_calls,
            };
            config.event_listeners.emit(&event);

            #[cfg(feature = "metrics")]
            {
                let wait_duration = acquire_start.elapsed();
                counter!("bulkhead_calls_permitted_total", "bulkhead" => config.name.clone())
                    .increment(1);
                gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                    .set(concurrent_calls as f64);
                histogram!("bulkhead_wait_duration_seconds", "bulkhead" => config.name.clone())
                    .record(wait_duration.as_secs_f64());
            }

            // Call the inner service
            let result = inner.call(request).await;

            // Drop the permit to release the slot
            drop(permit);

            let duration = start_time.elapsed();

            // Emit completion event
            match &result {
                Ok(_) => {
                    let event = BulkheadEvent::CallFinished {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        duration,
                    };
                    config.event_listeners.emit(&event);

                    #[cfg(feature = "metrics")]
                    {
                        counter!("bulkhead_calls_finished_total", "bulkhead" => config.name.clone())
                            .increment(1);
                        histogram!("bulkhead_call_duration_seconds", "bulkhead" => config.name.clone())
                            .record(duration.as_secs_f64());
                    }
                }
                Err(_) => {
                    let event = BulkheadEvent::CallFailed {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        duration,
                    };
                    config.event_listeners.emit(&event);

                    #[cfg(feature = "metrics")]
                    {
                        counter!("bulkhead_calls_failed_total", "bulkhead" => config.name.clone())
                            .increment(1);
                        histogram!("bulkhead_call_duration_seconds", "bulkhead" => config.name.clone())
                            .record(duration.as_secs_f64());
                    }
                }
            }

            #[cfg(feature = "metrics")]
            {
                let new_concurrent =
                    config.max_concurrent_calls - semaphore_for_check.available_permits();
                gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                    .set(new_concurrent as f64);
            }

            result.map_err(BulkheadServiceError::Inner)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BulkheadLayer;
    use futures::future::{pending, ready, Ready};
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tower::{service_fn, Layer, ServiceExt};

    #[derive(Clone)]
    struct ReadinessError;

    impl Service<()> for ReadinessError {
        type Response = ();
        type Error = &'static str;
        type Future = Ready<Result<(), Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Err("inner not ready"))
        }

        fn call(&mut self, (): ()) -> Self::Future {
            ready(Ok(()))
        }
    }

    #[test]
    fn accessors_expose_the_inner_service() {
        let layer = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .build()
            .unwrap();
        let mut bulkhead = layer.layer(service_fn(|()| async { Ok::<_, Infallible>(()) }));

        let _: &_ = bulkhead.get_ref();
        let _: &mut _ = bulkhead.get_mut();
        let _inner = bulkhead.into_inner();
    }

    #[tokio::test]
    async fn backpressure_call_requires_readiness_reservation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let layer = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build()
            .unwrap();
        let mut service = layer.layer(service_fn(move |()| {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, Infallible>(()) }
        }));

        let error = service.call(()).await.unwrap_err();
        assert!(matches!(
            error,
            BulkheadServiceError::Bulkhead(BulkheadError::NotReady)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn closed_backpressure_semaphore_is_a_readiness_error() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build_with_handle()
            .unwrap();
        let mut service = layer.layer(service_fn(|()| async { Ok::<_, Infallible>(()) }));
        handle.semaphore.close();

        let error = match service.ready().await {
            Ok(_) => panic!("closed semaphore unexpectedly reported readiness"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            BulkheadServiceError::Bulkhead(BulkheadError::Closed)
        ));
    }

    #[tokio::test]
    async fn inner_readiness_error_releases_reserved_permit() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build_with_handle()
            .unwrap();
        let mut service = layer.layer(ReadinessError);

        let error = match service.ready().await {
            Ok(_) => panic!("inner readiness error was not propagated"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            BulkheadServiceError::Inner("inner not ready")
        ));
        assert_eq!(handle.available_permits(), 1);
    }

    #[tokio::test]
    async fn dropping_reserved_clone_releases_exactly_one_permit() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build_with_handle()
            .unwrap();
        let service = layer.layer(service_fn(|()| async { Ok::<_, Infallible>(()) }));
        let mut reserved = service.clone();
        let mut waiter = service.clone();

        reserved.ready().await.unwrap();
        assert_eq!(handle.available_permits(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), waiter.ready())
                .await
                .is_err()
        );

        drop(reserved);
        tokio::time::timeout(Duration::from_secs(1), waiter.ready())
            .await
            .expect("dropping a readied clone must wake a waiter")
            .unwrap();
        assert_eq!(handle.available_permits(), 0);
        drop(waiter);
        assert_eq!(handle.available_permits(), 1);
    }

    #[tokio::test]
    async fn dropping_pending_waiter_does_not_block_fifo() {
        let layer = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build()
            .unwrap();
        let service = layer.layer(service_fn(|()| async { Ok::<_, Infallible>(()) }));
        let mut reserved = service.clone();
        let mut next = service.clone();
        reserved.ready().await.unwrap();

        let mut cancelled = service.clone();
        let waiter = tokio::spawn(async move { cancelled.ready().await.map(|_| ()) });
        tokio::task::yield_now().await;
        waiter.abort();
        let _ = waiter.await;

        drop(reserved);
        tokio::time::timeout(Duration::from_secs(1), next.ready())
            .await
            .expect("a dropped FIFO waiter must not retain queue position")
            .unwrap();
    }

    #[tokio::test]
    async fn dropping_call_future_releases_reserved_permit() {
        let (layer, handle) = BulkheadLayer::builder()
            .max_concurrent_calls(1)
            .backpressure()
            .build_with_handle()
            .unwrap();
        let service = layer.layer(service_fn(|()| pending::<Result<(), Infallible>>()));
        let mut first = service.clone();
        let mut second = service.clone();

        first.ready().await.unwrap();
        let future = first.call(());
        assert_eq!(handle.available_permits(), 0);
        drop(future);
        assert_eq!(handle.available_permits(), 1);

        tokio::time::timeout(Duration::from_secs(1), second.ready())
            .await
            .expect("dropping the response future must release its permit")
            .unwrap();
    }
}
