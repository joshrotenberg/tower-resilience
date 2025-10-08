//! Bulkhead service implementation.

use crate::config::BulkheadConfig;
use crate::error::BulkheadError;
use crate::events::BulkheadEvent;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::Semaphore;
use tower::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, gauge};

/// Bulkhead service that limits concurrent calls.
#[derive(Clone)]
pub struct Bulkhead<S> {
    inner: S,
    semaphore: Arc<Semaphore>,
    config: Arc<BulkheadConfig>,
}

impl<S> Bulkhead<S> {
    /// Creates a new bulkhead service.
    pub(crate) fn new(inner: S, config: BulkheadConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_calls));
        Self {
            inner,
            semaphore,
            config: Arc::new(config),
        }
    }
}

impl<S, Request> Service<Request> for Bulkhead<S>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: From<BulkheadError> + Send + 'static,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let semaphore = Arc::clone(&self.semaphore);
        let semaphore_for_check = Arc::clone(&self.semaphore);
        let config = Arc::clone(&self.config);
        let mut inner = self.inner.clone();
        let start_time = Instant::now();

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
                counter!("bulkhead_calls_permitted_total", "bulkhead" => config.name.clone())
                    .increment(1);
                gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                    .set(concurrent_calls as f64);
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
                    counter!("bulkhead_calls_finished_total", "bulkhead" => config.name.clone())
                        .increment(1);
                }
                Err(_) => {
                    let event = BulkheadEvent::CallFailed {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        duration,
                    };
                    config.event_listeners.emit(&event);

                    #[cfg(feature = "metrics")]
                    counter!("bulkhead_calls_failed_total", "bulkhead" => config.name.clone())
                        .increment(1);
                }
            }

            #[cfg(feature = "metrics")]
            {
                let new_concurrent =
                    config.max_concurrent_calls - semaphore_for_check.available_permits();
                gauge!("bulkhead_concurrent_calls", "bulkhead" => config.name.clone())
                    .set(new_concurrent as f64);
            }

            result
        })
    }
}
