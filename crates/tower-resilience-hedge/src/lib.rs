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
//! # Presets
//!
//! ```rust
//! use tower_resilience_hedge::HedgeLayer;
//!
//! let conservative = HedgeLayer::conservative(); // 500ms delay, 2 attempts
//! let standard = HedgeLayer::standard();         // 100ms delay, 3 attempts
//! let aggressive = HedgeLayer::aggressive();     // 50ms delay, 5 attempts
//! ```
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
//! // No type parameters needed! Fire a hedge request if primary takes > 100ms
//! let layer = HedgeLayer::builder()
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
//! // No type parameters needed! Fire 3 requests immediately, return fastest
//! let layer = HedgeLayer::builder()
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
//! // Wrap with hedging - fire hedge after 50ms (no type parameters needed!)
//! let hedge = HedgeLayer::builder()
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
//! Attempt futures are owned by the returned call future. When an attempt
//! succeeds, all losing futures are dropped before the success event is
//! emitted or the response is returned. Dropping the caller's future likewise
//! drops every attempt and emits no terminal success or failure event.
//!
//! Future cancellation is cooperative: dropping prevents further polling, but
//! it cannot undo side effects the downstream service already committed.
//!
//! # Eligibility and idempotency
//!
//! The default [`AlwaysHedge`] policy preserves historical behavior by
//! assuming every request is safe to execute more than once concurrently. For
//! a service that accepts both idempotent and non-idempotent operations,
//! configure a request predicate:
//!
//! ```rust
//! use tower_resilience_hedge::HedgeLayer;
//!
//! #[derive(Clone)]
//! struct Request {
//!     idempotent: bool,
//! }
//!
//! let layer = HedgeLayer::builder()
//!     .eligible_if(|request: &Request| request.idempotent)
//!     .max_hedged_attempts(3)
//!     .build();
//! ```
//!
//! An ineligible request executes exactly once as the primary and never emits
//! a [`HedgeEvent::HedgeStarted`] event. Eligibility should normally require
//! an intrinsically idempotent operation or a downstream idempotency key.
//!
//! # Type Requirements
//!
//! Hedging has specific trait bounds that differ from other resilience patterns:
//!
//! - **`Req: Clone`** - Required because the request is cloned to send parallel
//!   requests. Each hedge attempt needs its own copy of the request.
//!
//! - **`E: Clone`** - Required for error handling. When multiple attempts fail,
//!   errors need to be collected and stored to return the final error.
//!
//! If your request or error types don't implement `Clone`, consider:
//! - Wrapping them in `Arc` (e.g., `Arc<MyRequest>`)
//! - Using a different resilience pattern like Retry which doesn't require
//!   cloning requests

mod config;
mod error;
mod events;
mod layer;
mod policy;

pub use config::{HedgeConfig, HedgeConfigBuilder, HedgeDelay};
pub use error::HedgeError;
pub use events::HedgeEvent;
pub use layer::HedgeLayer;
pub use policy::{AlwaysHedge, HedgePolicy, HedgePredicate};

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower::{Service, ServiceExt};

/// Hedging service that wraps an inner service.
///
/// This service executes parallel redundant requests to reduce tail latency.
/// It fires additional "hedge" requests after a configurable delay and returns
/// whichever request completes first successfully.
///
/// The first type parameter is the inner service. The policy parameter defaults
/// to [`AlwaysHedge`] and is inferred when `eligible_if` is configured; request,
/// response, and error types are derived from the service implementation.
pub struct Hedge<S, P = AlwaysHedge> {
    inner: S,
    config: Arc<HedgeConfig>,
    policy: Arc<P>,
}

impl<S> Hedge<S, AlwaysHedge> {
    /// Create a new Hedge service with the given configuration.
    pub fn new(inner: S, config: HedgeConfig) -> Self {
        Self::with_policy(inner, config, Arc::new(AlwaysHedge))
    }
}

impl<S, P> Hedge<S, P> {
    pub(crate) fn with_policy(inner: S, config: HedgeConfig, policy: Arc<P>) -> Self {
        Self {
            inner,
            config: Arc::new(config),
            policy,
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

    /// Consumes the hedge, returning the inner service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: Clone, P> Clone for Hedge<S, P> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
            policy: Arc::clone(&self.policy),
        }
    }
}

impl<S, Req, P> Service<Req> for Hedge<S, P>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Response: Send + Sync + 'static,
    S::Error: Clone + Send + Sync + 'static,
    S::Future: Send,
    Req: Clone + Send + Sync + 'static,
    P: HedgePolicy<Req>,
{
    type Response = S::Response;
    type Error = HedgeError<S::Error>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(HedgeError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let config = Arc::clone(&self.config);
        let policy = Arc::clone(&self.policy);
        let inner = self.inner.clone();
        // Replace the clone we just made with the ready service
        let inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move { execute_with_hedging(inner, req, config, policy).await })
    }
}

type AttemptFuture<R, E> = BoxFuture<'static, (usize, Result<R, E>)>;

fn attempt_future<S, Req>(
    mut service: S,
    request: Req,
    attempt: usize,
    already_ready: bool,
) -> AttemptFuture<S::Response, S::Error>
where
    S: Service<Req> + Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send,
    Req: Send + 'static,
{
    Box::pin(async move {
        let result = if already_ready {
            service.call(request).await
        } else {
            // Clones do not inherit readiness. Every hedge must drive its own
            // clone to readiness before consuming it with `call`. See #293.
            match service.ready().await {
                Ok(service) => service.call(request).await,
                Err(error) => Err(error),
            }
        };
        (attempt, result)
    })
}

/// Execute the request with the configured hedging strategy.
async fn execute_with_hedging<S, Req, P>(
    service: S,
    req: Req,
    config: Arc<HedgeConfig>,
    policy: Arc<P>,
) -> Result<S::Response, HedgeError<S::Error>>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Response: Send + 'static,
    S::Error: Clone + Send + 'static,
    S::Future: Send,
    Req: Clone + Send + 'static,
    P: HedgePolicy<Req>,
{
    // An ineligible request follows the same one-attempt success/error path as
    // `max_hedged_attempts(1)`, but is never cloned or fanned out.
    let max_attempts = if policy.is_eligible(&req) {
        config.max_hedged_attempts
    } else {
        1
    };
    let start = Instant::now();

    // Emit primary started event
    config.listeners.emit(&HedgeEvent::PrimaryStarted {
        name: config.name.clone(),
        timestamp: Instant::now(),
    });

    // Keep every attempt future owned by this call future. Dropping the caller
    // future or this collection cancels work synchronously; no detached Tokio
    // tasks survive a winner or caller cancellation.
    let mut attempts: FuturesUnordered<AttemptFuture<S::Response, S::Error>> =
        FuturesUnordered::new();

    let hedge_template = (max_attempts > 1).then(|| service.clone());
    let mut request = Some(req);
    let primary_request = if max_attempts > 1 {
        request
            .as_ref()
            .expect("hedged request must be available")
            .clone()
    } else {
        request.take().expect("primary request must be available")
    };
    attempts.push(attempt_future(service, primary_request, 0, true));

    let mut hedges_started = 0usize;
    let mut hedges_in_flight = 0usize;
    let mut primary_in_flight = true;
    let mut primary_error: Option<S::Error> = None;
    let mut first_hedge_error: Option<S::Error> = None;

    let first_delay = config.delay.get_delay(1);
    let mut delay_future = match first_delay {
        Some(delay) if max_attempts > 1 && delay > Duration::ZERO => {
            Some(Box::pin(tokio::time::sleep(delay)))
        }
        _ => None,
    };

    // A zero/absent first delay is parallel mode: register every attempt
    // before polling for a result.
    if max_attempts > 1 && delay_future.is_none() {
        for attempt in 1..max_attempts {
            config.listeners.emit(&HedgeEvent::HedgeStarted {
                name: config.name.clone(),
                attempt,
                delay: Duration::ZERO,
                timestamp: Instant::now(),
            });
            attempts.push(attempt_future(
                hedge_template
                    .as_ref()
                    .expect("hedge service template must be available")
                    .clone(),
                request
                    .as_ref()
                    .expect("hedged request must be available")
                    .clone(),
                attempt,
                false,
            ));
            hedges_started += 1;
            hedges_in_flight += 1;
        }
    }

    enum RaceResult<R, E> {
        Attempt(usize, Result<R, E>),
        StartHedge,
    }

    while !attempts.is_empty() || hedges_started + 1 < max_attempts {
        let next = if let Some(delay) = delay_future.as_mut() {
            tokio::select! {
                biased;
                result = attempts.next(), if !attempts.is_empty() => {
                    let (attempt, result) = result.expect("an in-flight attempt must resolve");
                    RaceResult::Attempt(attempt, result)
                }
                _ = delay.as_mut() => RaceResult::StartHedge,
            }
        } else {
            let (attempt, result) = attempts
                .next()
                .await
                .expect("at least one attempt must remain");
            RaceResult::Attempt(attempt, result)
        };

        match next {
            RaceResult::StartHedge => {
                hedges_started += 1;
                hedges_in_flight += 1;
                let attempt = hedges_started;
                let elapsed_delay = config
                    .delay
                    .get_delay(attempt)
                    .expect("hedge delays are defined for every attempt");

                config.listeners.emit(&HedgeEvent::HedgeStarted {
                    name: config.name.clone(),
                    attempt,
                    delay: elapsed_delay,
                    timestamp: Instant::now(),
                });
                attempts.push(attempt_future(
                    hedge_template
                        .as_ref()
                        .expect("hedge service template must be available")
                        .clone(),
                    request
                        .as_ref()
                        .expect("hedged request must be available")
                        .clone(),
                    attempt,
                    false,
                ));

                delay_future = if hedges_started + 1 < max_attempts {
                    config
                        .delay
                        .get_delay(hedges_started + 1)
                        .map(|delay| Box::pin(tokio::time::sleep(delay)))
                } else {
                    None
                };
            }
            RaceResult::Attempt(attempt, result) => {
                if attempt == 0 {
                    primary_in_flight = false;
                } else {
                    hedges_in_flight = hedges_in_flight.saturating_sub(1);
                }

                let response = match result {
                    Ok(response) => response,
                    Err(error) => {
                        if attempt == 0 {
                            primary_error = Some(error);
                        } else if first_hedge_error.is_none() {
                            first_hedge_error = Some(error);
                        }
                        continue;
                    }
                };

                let duration = start.elapsed();
                let hedges_cancelled = hedges_in_flight;
                let primary_cancelled = primary_in_flight;

                // Cancellation is synchronous: event listeners observing the
                // terminal success see every losing call future already
                // dropped, and no loser can subsequently report completion.
                drop(attempts);
                drop(delay_future);

                if attempt == 0 {
                    config.listeners.emit(&HedgeEvent::PrimarySucceeded {
                        name: config.name.clone(),
                        duration,
                        hedges_cancelled,
                        timestamp: Instant::now(),
                    });
                } else {
                    config.listeners.emit(&HedgeEvent::HedgeSucceeded {
                        name: config.name.clone(),
                        attempt,
                        duration,
                        primary_cancelled,
                        timestamp: Instant::now(),
                    });
                }
                return Ok(response);
            }
        }
    }

    // All attempts failed
    config.listeners.emit(&HedgeEvent::AllFailed {
        name: config.name.clone(),
        attempts: max_attempts,
        timestamp: Instant::now(),
    });

    Err(HedgeError::AllAttemptsFailed(
        primary_error
            .or(first_hedge_error)
            .expect("all completed attempts must yield at least one error"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::{Layer, ServiceExt};

    #[derive(Clone, Debug)]
    struct TestError;

    #[test]
    fn accessors_expose_the_inner_service() {
        let layer = HedgeLayer::builder()
            .delay(Duration::from_millis(100))
            .build();
        let mut service = layer.layer(tower::service_fn(|_req: String| async {
            Ok::<_, TestError>(())
        }));

        let _: &_ = service.get_ref();
        let _: &mut _ = service.get_mut();
        let _inner = service.into_inner();
    }

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

        // No type parameters needed!
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

        // No type parameters needed!
        let layer = HedgeLayer::builder()
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

        // Every parallel attempt was polled before the first one completed.
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

        // No type parameters needed!
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
        // Hedge fires at 50ms and the fast hedge service wins; upper bound is
        // generous (~3x expected) for CI scheduling slop. See #301.
        assert!(elapsed < Duration::from_millis(300));

        // Both were called; the slow primary was dropped before return.
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_all_fail_returns_error() {
        let service = tower::service_fn(|_req: String| async move { Err::<String, _>(TestError) });

        // No type parameters needed!
        let layer = HedgeLayer::builder()
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

    #[test]
    fn test_preset_conservative() {
        let _layer = HedgeLayer::conservative();
    }

    #[test]
    fn test_preset_standard() {
        let _layer = HedgeLayer::standard();
    }

    #[test]
    fn test_preset_aggressive() {
        let _layer = HedgeLayer::aggressive();
    }
}
