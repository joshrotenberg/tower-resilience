//! Hedging middleware for Tower services.
//!
//! Hedging reduces tail latency by executing parallel redundant requests.
//! Instead of waiting for a slow request to complete, hedging fires additional
//! requests after a configurable delay and returns whichever completes first.
//!
//! # Overview
//!
//! The hedging pattern is useful when:
//! - Tail latency (P99/P999) is critical
//! - Operations are idempotent and safe to retry
//! - You can trade increased resource usage for lower latency
//!
//! # Modes
//!
//! ## Latency Mode (delay > 0)
//!
//! Wait a specified duration before firing hedge requests. This is the default
//! and most common mode - it only sends extra requests if the primary is slow.
//!
//! ```rust,no_run
//! use tower_resilience_hedge::HedgeLayer;
//! use std::time::Duration;
//!
//! // Fire a hedge request if primary takes > 100ms
//! let layer = HedgeLayer::<(), String, std::io::Error>::builder()
//!     .delay(Duration::from_millis(100))
//!     .max_hedged_attempts(2)
//!     .build();
//! ```
//!
//! ## Parallel Mode (delay = 0)
//!
//! Fire all requests simultaneously and return the fastest response.
//! Use when latency is critical and you can afford the resource cost.
//!
//! ```rust,no_run
//! use tower_resilience_hedge::HedgeLayer;
//!
//! // Fire 3 requests immediately, return fastest
//! let layer = HedgeLayer::<(), String, std::io::Error>::builder()
//!     .no_delay()
//!     .max_hedged_attempts(3)
//!     .build();
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! use tower::{Service, ServiceExt, Layer};
//! use tower_resilience_hedge::HedgeLayer;
//! use std::time::Duration;
//!
//! // Define a simple cloneable error type
//! #[derive(Clone, Debug)]
//! struct MyError;
//! impl std::fmt::Display for MyError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "MyError")
//!     }
//! }
//! impl std::error::Error for MyError {}
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a service that sometimes responds slowly
//! let service = tower::service_fn(|req: String| async move {
//!     // Simulate variable latency
//!     Ok::<_, MyError>(format!("response: {}", req))
//! });
//!
//! // Wrap with hedging - fire hedge after 50ms
//! let hedge = HedgeLayer::<String, String, MyError>::builder()
//!     .delay(Duration::from_millis(50))
//!     .max_hedged_attempts(2)
//!     .build();
//!
//! let mut service = hedge.layer(service);
//!
//! let response = service.ready().await?.call("hello".to_string()).await?;
//! println!("Got response: {}", response);
//! # Ok(())
//! # }
//! ```
//!
//! # Cancellation
//!
//! When one request succeeds, all other in-flight requests are cancelled
//! by dropping their futures. This relies on the inner service supporting
//! cooperative cancellation.

mod config;
mod error;
mod events;
mod layer;

pub use config::{HedgeConfig, HedgeConfigBuilder, HedgeDelay};
pub use error::HedgeError;
pub use events::HedgeEvent;
pub use layer::HedgeLayer;

use futures::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower::Service;

/// Hedging service that wraps an inner service.
///
/// This service executes parallel redundant requests to reduce tail latency.
/// It fires additional "hedge" requests after a configurable delay and returns
/// whichever request completes first successfully.
pub struct Hedge<S, Req, Res, E> {
    inner: S,
    config: Arc<HedgeConfig<Req, Res, E>>,
}

impl<S, Req, Res, E> Hedge<S, Req, Res, E> {
    /// Create a new Hedge service with the given configuration.
    pub fn new(inner: S, config: HedgeConfig<Req, Res, E>) -> Self {
        Self {
            inner,
            config: Arc::new(config),
        }
    }
}

impl<S: Clone, Req, Res, E> Clone for Hedge<S, Req, Res, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
        }
    }
}

impl<S, Req, Res, E> Service<Req> for Hedge<S, Req, Res, E>
where
    S: Service<Req, Response = Res, Error = E> + Clone + Send + 'static,
    S::Future: Send,
    Req: Clone + Send + Sync + 'static,
    Res: Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    type Response = Res;
    type Error = HedgeError<E>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(HedgeError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let config = Arc::clone(&self.config);
        let inner = self.inner.clone();
        // Replace the clone we just made with the ready service
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move { execute_with_hedging(inner, req, config).await })
    }
}

/// Execute the request with hedging strategy
async fn execute_with_hedging<S, Req, Res, E>(
    service: S,
    req: Req,
    config: Arc<HedgeConfig<Req, Res, E>>,
) -> Result<Res, HedgeError<E>>
where
    S: Service<Req, Response = Res, Error = E> + Clone + Send + 'static,
    S::Future: Send,
    Req: Clone + Send + 'static,
    Res: Send + 'static,
    E: Clone + Send + 'static,
{
    use tokio::sync::mpsc;

    let max_attempts = config.max_hedged_attempts;
    let start = Instant::now();

    // Emit primary started event
    config.listeners.emit(&HedgeEvent::PrimaryStarted {
        name: config.name.clone(),
        timestamp: Instant::now(),
    });

    // Channel to collect results from all attempts
    let (tx, mut rx) = mpsc::channel::<(usize, Result<Res, E>)>(max_attempts);

    // Spawn primary request
    let mut service_clone = service.clone();
    let req_clone = req.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let result = service_clone.call(req_clone).await;
        let _ = tx_clone.send((0, result)).await;
    });

    // Track spawned hedge tasks
    let mut hedges_spawned: usize = 0;
    let mut primary_error: Option<E> = None;

    // Get delay for first hedge
    let first_delay = config.delay.get_delay(1);

    // If we have more attempts and there's a delay, set up hedge timing
    if max_attempts > 1 {
        match first_delay {
            Some(delay) if delay > Duration::ZERO => {
                // Latency mode: wait for delay or result
                let mut delay_fut = std::pin::pin!(tokio::time::sleep(delay));

                loop {
                    tokio::select! {
                        biased;

                        // Check for results
                        Some((attempt, result)) = rx.recv() => {
                            match &result {
                                Ok(_) => {
                                    let duration = start.elapsed();
                                    if attempt == 0 {
                                        config.listeners.emit(&HedgeEvent::PrimarySucceeded {
                                            name: config.name.clone(),
                                            duration,
                                            hedges_cancelled: hedges_spawned,
                                            timestamp: Instant::now(),
                                        });
                                    } else {
                                        config.listeners.emit(&HedgeEvent::HedgeSucceeded {
                                            name: config.name.clone(),
                                            attempt,
                                            duration,
                                            primary_cancelled: true,
                                            timestamp: Instant::now(),
                                        });
                                    }
                                    return result.map_err(HedgeError::Inner);
                                }
                                Err(e) => {
                                    // Store error, continue waiting for other attempts
                                    if attempt == 0 {
                                        primary_error = Some(e.clone());
                                    }
                                    // Check if all attempts exhausted
                                    if hedges_spawned + 1 >= max_attempts {
                                        // All spawned, check if this was the last result
                                        config.listeners.emit(&HedgeEvent::AllFailed {
                                            name: config.name.clone(),
                                            attempts: hedges_spawned + 1,
                                            timestamp: Instant::now(),
                                        });
                                        return Err(HedgeError::AllAttemptsFailed(
                                            primary_error.unwrap_or_else(|| e.clone())
                                        ));
                                    }
                                }
                            }
                        }

                        // Delay elapsed, spawn hedge
                        _ = &mut delay_fut, if hedges_spawned + 1 < max_attempts => {
                            hedges_spawned += 1;
                            let attempt_num = hedges_spawned;

                            config.listeners.emit(&HedgeEvent::HedgeStarted {
                                name: config.name.clone(),
                                attempt: attempt_num,
                                delay,
                                timestamp: Instant::now(),
                            });

                            let mut svc = service.clone();
                            let r = req.clone();
                            let tx_c = tx.clone();
                            tokio::spawn(async move {
                                let result = svc.call(r).await;
                                let _ = tx_c.send((attempt_num, result)).await;
                            });

                            // Set up next delay if more hedges available
                            if hedges_spawned + 1 < max_attempts {
                                if let Some(next_delay) = config.delay.get_delay(hedges_spawned + 1) {
                                    delay_fut.set(tokio::time::sleep(next_delay));
                                }
                            }
                        }

                        else => {
                            // No more hedges to spawn, just wait for results
                            if let Some((attempt, result)) = rx.recv().await {
                                match &result {
                                    Ok(_) => {
                                        let duration = start.elapsed();
                                        if attempt == 0 {
                                            config.listeners.emit(&HedgeEvent::PrimarySucceeded {
                                                name: config.name.clone(),
                                                duration,
                                                hedges_cancelled: hedges_spawned,
                                                timestamp: Instant::now(),
                                            });
                                        } else {
                                            config.listeners.emit(&HedgeEvent::HedgeSucceeded {
                                                name: config.name.clone(),
                                                attempt,
                                                duration,
                                                primary_cancelled: attempt != 0,
                                                timestamp: Instant::now(),
                                            });
                                        }
                                        return result.map_err(HedgeError::Inner);
                                    }
                                    Err(e) => {
                                        if attempt == 0 && primary_error.is_none() {
                                            primary_error = Some(e.clone());
                                        }
                                    }
                                }
                            } else {
                                // Channel closed, all senders dropped
                                break;
                            }
                        }
                    }
                }
            }
            _ => {
                // Parallel mode: spawn all hedges immediately
                for i in 1..max_attempts {
                    hedges_spawned += 1;

                    config.listeners.emit(&HedgeEvent::HedgeStarted {
                        name: config.name.clone(),
                        attempt: i,
                        delay: Duration::ZERO,
                        timestamp: Instant::now(),
                    });

                    let mut svc = service.clone();
                    let r = req.clone();
                    let tx_c = tx.clone();
                    tokio::spawn(async move {
                        let result = svc.call(r).await;
                        let _ = tx_c.send((i, result)).await;
                    });
                }
            }
        }
    }

    // Drop our sender so channel closes when all tasks complete
    drop(tx);

    // Wait for first success or all failures
    let mut attempts_received: usize = 0;
    let total_attempts = hedges_spawned + 1;

    while let Some((attempt, result)) = rx.recv().await {
        attempts_received += 1;

        match result {
            Ok(res) => {
                let duration = start.elapsed();
                if attempt == 0 {
                    config.listeners.emit(&HedgeEvent::PrimarySucceeded {
                        name: config.name.clone(),
                        duration,
                        hedges_cancelled: hedges_spawned.saturating_sub(attempts_received - 1),
                        timestamp: Instant::now(),
                    });
                } else {
                    config.listeners.emit(&HedgeEvent::HedgeSucceeded {
                        name: config.name.clone(),
                        attempt,
                        duration,
                        primary_cancelled: true,
                        timestamp: Instant::now(),
                    });
                }
                return Ok(res);
            }
            Err(e) => {
                if primary_error.is_none() {
                    primary_error = Some(e);
                }
            }
        }
    }

    // All attempts failed
    config.listeners.emit(&HedgeEvent::AllFailed {
        name: config.name.clone(),
        attempts: total_attempts,
        timestamp: Instant::now(),
    });

    Err(HedgeError::AllAttemptsFailed(
        primary_error.expect("at least one error should exist"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::{Layer, ServiceExt};

    #[derive(Clone, Debug)]
    struct TestError;

    #[tokio::test]
    async fn test_primary_succeeds_no_hedge() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>("success".to_string())
            }
        });

        let layer = HedgeLayer::builder()
            .delay(Duration::from_millis(100))
            .max_hedged_attempts(2)
            .build();

        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        assert!(result.is_ok());

        // Give a moment for any hedges to complete
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Should only have called once since primary was fast
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_parallel_mode_all_called() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok::<_, TestError>("success".to_string())
            }
        });

        let layer = HedgeLayer::<String, String, TestError>::builder()
            .no_delay()
            .max_hedged_attempts(3)
            .build();

        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        assert!(result.is_ok());

        // Give time for all spawned tasks to increment counter
        tokio::time::sleep(Duration::from_millis(100)).await;

        // All 3 should have been called in parallel mode
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_hedge_fires_after_delay() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                let count = cc.fetch_add(1, Ordering::SeqCst);
                // First call is slow, second is fast
                if count == 0 {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Ok::<_, TestError>("success".to_string())
            }
        });

        let layer = HedgeLayer::builder()
            .delay(Duration::from_millis(50))
            .max_hedged_attempts(2)
            .build();

        let mut service = layer.layer(service);

        let start = Instant::now();
        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        // Should complete faster than 200ms because hedge succeeded
        assert!(elapsed < Duration::from_millis(150));

        // Both should have been called
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_all_fail_returns_error() {
        let service = tower::service_fn(|_req: String| async move { Err::<String, _>(TestError) });

        let layer = HedgeLayer::<String, String, TestError>::builder()
            .no_delay()
            .max_hedged_attempts(2)
            .build();

        let mut service = layer.layer(service);

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        assert!(matches!(result, Err(HedgeError::AllAttemptsFailed(_))));
    }
}
