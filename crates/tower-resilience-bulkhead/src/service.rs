//! Bulkhead service implementation.

use crate::config::BulkheadConfig;
use crate::error::{BulkheadError, BulkheadServiceError};
use crate::events::BulkheadEvent;
use futures::future::BoxFuture;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, gauge, histogram};

type AcquireFuture =
    Pin<Box<dyn Future<Output = Result<OwnedSemaphorePermit, tokio::sync::AcquireError>> + Send>>;

/// Bulkhead service that limits concurrent calls.
pub struct Bulkhead<S> {
    inner: S,
    semaphore: Arc<Semaphore>,
    config: Arc<BulkheadConfig>,
    /// Permit reserved in `poll_ready` (backpressure mode only).
    permit: Option<OwnedSemaphorePermit>,
    /// In-flight semaphore acquire future (backpressure mode only).
    acquire_future: Option<AcquireFuture>,
}

impl<S: Clone> Clone for Bulkhead<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            semaphore: Arc::clone(&self.semaphore),
            config: Arc::clone(&self.config),
            permit: None,
            acquire_future: None,
        }
    }
}

impl<S> Bulkhead<S> {
    /// Creates a new bulkhead service.
    pub(crate) fn new(inner: S, config: BulkheadConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_calls));
        Self {
            inner,
            semaphore,
            config: Arc::new(config),
            permit: None,
            acquire_future: None,
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
            semaphore,
            config,
            permit: None,
            acquire_future: None,
        }
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
        // Check inner service readiness first
        match self.inner.poll_ready(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(BulkheadServiceError::Inner(e))),
            Poll::Ready(Ok(())) => {}
        }

        if !self.config.backpressure {
            return Poll::Ready(Ok(()));
        }

        // Backpressure mode: acquire permit in poll_ready
        if self.permit.is_some() {
            return Poll::Ready(Ok(()));
        }

        // Create or poll the acquire future
        let fut = self.acquire_future.get_or_insert_with(|| {
            let sem = Arc::clone(&self.semaphore);
            Box::pin(async move { sem.acquire_owned().await })
        });

        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok(permit)) => {
                self.acquire_future = None;
                self.permit = Some(permit);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(_)) => {
                // Semaphore closed -- should not happen in normal operation
                self.acquire_future = None;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if let Some(permit) = self.permit.take() {
            // Backpressure mode: permit already acquired in poll_ready
            let semaphore_for_check = Arc::clone(&self.semaphore);
            let config = Arc::clone(&self.config);
            let mut inner = self.inner.clone();
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
        let mut inner = self.inner.clone();
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
                            // Semaphore was closed, shouldn't happen in normal operation
                            let event = BulkheadEvent::CallRejected {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                max_concurrent_calls: config.max_concurrent_calls,
                            };
                            config.event_listeners.emit(&event);

                            #[cfg(feature = "metrics")]
                            counter!("bulkhead_calls_rejected_total", "bulkhead" => config.name.clone())
                                .increment(1);

                            return Err(BulkheadError::BulkheadFull {
                                max_concurrent_calls: config.max_concurrent_calls,
                            }
                            .into());
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

                            return Err(BulkheadError::BulkheadFull {
                                max_concurrent_calls: config.max_concurrent_calls,
                            }
                            .into());
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
