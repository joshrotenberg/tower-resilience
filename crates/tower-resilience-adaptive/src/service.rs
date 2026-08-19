//! Service implementation for adaptive concurrency limiting.

use crate::ConcurrencyAlgorithm;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::PollSemaphore;
use tower_service::Service;

#[cfg(feature = "tracing")]
use tracing::debug;

#[derive(Debug)]
struct CapacityState {
    desired_limit: usize,
    shrink_debt: usize,
}

/// A service that applies adaptive concurrency limiting.
///
/// This service dynamically adjusts the number of concurrent requests based
/// on observed latency and error rates. A successful [`Service::poll_ready`]
/// reserves one shared permit for that clone. Dropping the clone or the future
/// returned by [`Service::call`] releases the reservation and wakes a waiter.
pub struct AdaptiveService<S, A> {
    inner: S,
    algorithm: Arc<A>,
    /// Serializes changes to the semaphore's logical capacity.
    capacity: Arc<Mutex<CapacityState>>,
    /// In-flight requests counter
    in_flight: Arc<AtomicUsize>,
    /// Semaphore for limiting concurrency
    semaphore: Arc<Semaphore>,
    /// Pollable acquisition state local to this clone.
    poll_semaphore: PollSemaphore,
    /// Capacity reserved by a successful `poll_ready` call.
    permit: Option<OwnedSemaphorePermit>,
}

impl<S, A> AdaptiveService<S, A>
where
    A: ConcurrencyAlgorithm,
{
    /// Create a new adaptive service.
    pub fn new(service: S, algorithm: Arc<A>) -> Self {
        let initial_limit = algorithm.limit();
        let semaphore = Arc::new(Semaphore::new(initial_limit));
        Self {
            inner: service,
            algorithm,
            capacity: Arc::new(Mutex::new(CapacityState {
                desired_limit: initial_limit,
                shrink_debt: 0,
            })),
            in_flight: Arc::new(AtomicUsize::new(0)),
            semaphore: Arc::clone(&semaphore),
            poll_semaphore: PollSemaphore::new(semaphore),
            permit: None,
        }
    }

    /// Get the current concurrency limit.
    pub fn limit(&self) -> usize {
        self.algorithm.limit()
    }

    /// Get the number of in-flight requests.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Get a reference to the algorithm.
    pub fn algorithm(&self) -> &A {
        &self.algorithm
    }
}

impl<S, A> AdaptiveService<S, A> {
    /// Returns a reference to the inner service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Returns a mutable reference to the inner service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    // No `into_inner`: `AdaptiveService` has a custom `Drop` impl (below) that
    // releases a reserved semaphore permit, and Rust's partial-move rules
    // (E0509) forbid moving a field out of a type with a manual `Drop` impl.
    // Working around that would require `unsafe` (`ManuallyDrop` plus a
    // hand-written re-implementation of `Drop::drop` for the remaining
    // fields) for a single accessor method; not worth the risk here. This is
    // the one wrapper in the crate set without the full `get_ref`/`get_mut`/
    // `into_inner` triad -- see `docs/tower-api-surface-audit.md`.
}

impl<S, A> Clone for AdaptiveService<S, A>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            algorithm: Arc::clone(&self.algorithm),
            capacity: Arc::clone(&self.capacity),
            in_flight: Arc::clone(&self.in_flight),
            semaphore: Arc::clone(&self.semaphore),
            poll_semaphore: PollSemaphore::new(Arc::clone(&self.semaphore)),
            permit: None,
        }
    }
}

impl<S, A> Drop for AdaptiveService<S, A> {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            release_permit(permit, &self.capacity);
        }
    }
}

fn sync_limit<A>(algorithm: &A, semaphore: &Semaphore, capacity: &Mutex<CapacityState>)
where
    A: ConcurrencyAlgorithm + ?Sized,
{
    let mut state = capacity.lock().expect("adaptive capacity lock poisoned");
    let limit = algorithm.limit();

    if limit > state.desired_limit {
        #[cfg(feature = "tracing")]
        let old_limit = state.desired_limit;

        let increase = limit - state.desired_limit;
        let cancelled_debt = increase.min(state.shrink_debt);
        state.shrink_debt -= cancelled_debt;
        semaphore.add_permits(increase - cancelled_debt);

        #[cfg(feature = "tracing")]
        debug!(
            old_limit,
            new_limit = limit,
            direction = "increase",
            "Adaptive concurrency limit changed"
        );

        #[cfg(feature = "metrics")]
        {
            metrics::counter!("adaptive_limit_changes_total", "direction" => "increase")
                .increment(1);
            metrics::gauge!("adaptive_limit").set(limit as f64);
        }
    } else if limit < state.desired_limit {
        #[cfg(feature = "tracing")]
        let old_limit = state.desired_limit;

        let decrease = state.desired_limit - limit;
        let removed = semaphore.forget_permits(decrease);
        state.shrink_debt += decrease - removed;

        #[cfg(feature = "tracing")]
        debug!(
            old_limit,
            new_limit = limit,
            direction = "decrease",
            "Adaptive concurrency limit changed"
        );

        #[cfg(feature = "metrics")]
        {
            metrics::counter!("adaptive_limit_changes_total", "direction" => "decrease")
                .increment(1);
            metrics::gauge!("adaptive_limit").set(limit as f64);
        }
    }

    state.desired_limit = limit;
}

fn release_permit(permit: OwnedSemaphorePermit, capacity: &Mutex<CapacityState>) {
    let retire = {
        let mut state = capacity.lock().expect("adaptive capacity lock poisoned");
        if state.shrink_debt == 0 {
            false
        } else {
            state.shrink_debt -= 1;
            true
        }
    };

    if retire {
        permit.forget();
    }
}

struct AdmissionGuard<A: ConcurrencyAlgorithm> {
    algorithm: Arc<A>,
    capacity: Arc<Mutex<CapacityState>>,
    in_flight: Arc<AtomicUsize>,
    semaphore: Arc<Semaphore>,
    permit: Option<OwnedSemaphorePermit>,
    completed: bool,
}

impl<A> AdmissionGuard<A>
where
    A: ConcurrencyAlgorithm,
{
    fn complete(mut self) {
        self.completed = true;
    }
}

impl<A> Drop for AdmissionGuard<A>
where
    A: ConcurrencyAlgorithm,
{
    fn drop(&mut self) {
        if !self.completed {
            self.algorithm.record_dropped();
            sync_limit(self.algorithm.as_ref(), &self.semaphore, &self.capacity);
        }

        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if let Some(permit) = self.permit.take() {
            release_permit(permit, &self.capacity);
        }
    }
}

impl<S, A, Req> Service<Req> for AdaptiveService<S, A>
where
    S: Service<Req>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    A: ConcurrencyAlgorithm + 'static,
{
    type Response = S::Response;
    type Error = AdaptiveError<S::Error>;
    type Future = AdaptiveFuture<S::Response, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        sync_limit(self.algorithm.as_ref(), &self.semaphore, &self.capacity);

        if self.permit.is_none() {
            match self.poll_semaphore.poll_acquire(cx) {
                Poll::Ready(Some(permit)) => self.permit = Some(permit),
                Poll::Ready(None) => return Poll::Ready(Err(AdaptiveError::LimitReached)),
                Poll::Pending => return Poll::Pending,
            }
        }

        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => {
                if let Some(permit) = self.permit.take() {
                    release_permit(permit, &self.capacity);
                }
                Poll::Ready(Err(AdaptiveError::Service(error)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let permit = self
            .permit
            .take()
            .expect("AdaptiveService::call requires a successful poll_ready reservation");
        let start = Instant::now();
        self.in_flight.fetch_add(1, Ordering::SeqCst);

        let algorithm = Arc::clone(&self.algorithm);
        let semaphore = Arc::clone(&self.semaphore);
        let capacity = Arc::clone(&self.capacity);
        let guard = AdmissionGuard {
            algorithm: Arc::clone(&algorithm),
            capacity: Arc::clone(&capacity),
            in_flight: Arc::clone(&self.in_flight),
            semaphore: Arc::clone(&semaphore),
            permit: Some(permit),
            completed: false,
        };
        let future = self.inner.call(req);

        AdaptiveFuture {
            inner: Box::pin(async move {
                let result = future.await;
                let latency = start.elapsed();

                #[cfg(feature = "metrics")]
                metrics::histogram!("adaptive_rtt_seconds").record(latency.as_secs_f64());

                match &result {
                    Ok(_) => algorithm.record_success(latency),
                    Err(_) => algorithm.record_failure(),
                }

                sync_limit(algorithm.as_ref(), &semaphore, &capacity);
                guard.complete();

                result.map_err(AdaptiveError::Service)
            }),
        }
    }
}

/// Error type for adaptive limiter.
#[derive(Debug)]
pub enum AdaptiveError<E> {
    /// The service returned an error.
    Service(E),
    /// The concurrency limit was reached.
    LimitReached,
}

impl<E: std::fmt::Display> std::fmt::Display for AdaptiveError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service(e) => write!(f, "service error: {}", e),
            Self::LimitReached => write!(f, "concurrency limit reached"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for AdaptiveError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Service(e) => Some(e),
            Self::LimitReached => None,
        }
    }
}

/// Future returned by [`AdaptiveService`].
pub struct AdaptiveFuture<T, E> {
    inner: Pin<Box<dyn Future<Output = Result<T, AdaptiveError<E>>> + Send>>,
}

impl<T, E> Future for AdaptiveFuture<T, E> {
    type Output = Result<T, AdaptiveError<E>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Aimd;
    use std::time::Duration;

    #[test]
    fn accessors_expose_the_inner_service() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });
        let algorithm = Aimd::builder().initial_limit(10).build().unwrap();
        let mut service = AdaptiveService::new(service, Arc::new(algorithm));

        // No `into_inner`: see the comment on the accessor impl block.
        let _: &_ = service.get_ref();
        let _: &mut _ = service.get_mut();
    }

    #[tokio::test]
    async fn test_service_basic() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        let algorithm = Aimd::builder()
            .initial_limit(10)
            .latency_threshold(Duration::from_secs(1))
            .build()
            .unwrap();

        let mut service = AdaptiveService::new(service, Arc::new(algorithm));

        use tower::ServiceExt;
        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn successful_calls_emit_metrics() {
        use metrics::set_global_recorder;
        use metrics_util::debugging::{DebugValue, DebuggingRecorder};
        use std::sync::LazyLock;

        static RECORDER: LazyLock<DebuggingRecorder> = LazyLock::new(DebuggingRecorder::default);
        let _ = set_global_recorder(&*RECORDER);

        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        // A fast, well-under-threshold success grows the AIMD limit by
        // `increase_by` (1 by default), guaranteeing a limit-change event.
        let algorithm = Aimd::builder()
            .initial_limit(10)
            .increase_by(1)
            .latency_threshold(Duration::from_secs(1))
            .build()
            .unwrap();
        let mut service = AdaptiveService::new(service, Arc::new(algorithm));

        use tower::ServiceExt;
        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
        assert_eq!(service.limit(), 11);

        let snapshot = RECORDER.snapshotter().snapshot().into_vec();

        let increase_recorded = snapshot.iter().any(|(key, _, _, value)| {
            key.key().name() == "adaptive_limit_changes_total"
                && matches!(value, DebugValue::Counter(v) if *v >= 1)
                && key
                    .key()
                    .labels()
                    .any(|label| label.key() == "direction" && label.value() == "increase")
        });
        let rtt_recorded = snapshot
            .iter()
            .any(|(key, _, _, _)| key.key().name() == "adaptive_rtt_seconds");

        assert!(
            increase_recorded,
            "expected adaptive_limit_changes_total{{direction=\"increase\"}} > 0"
        );
        assert!(
            rtt_recorded,
            "expected an adaptive_rtt_seconds histogram entry"
        );
    }

    #[tokio::test]
    async fn test_in_flight_tracking() {
        let service = tower::service_fn(|_req: ()| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, &str>(())
        });

        let algorithm = Aimd::builder().initial_limit(10).build().unwrap();
        let service = AdaptiveService::new(service, Arc::new(algorithm));

        assert_eq!(service.in_flight(), 0);

        // Start a request
        let mut svc = service.clone();
        use tower::ServiceExt;
        let fut = svc.ready().await.unwrap().call(());

        // In-flight should be 1
        assert_eq!(service.in_flight(), 1);

        // Complete the request
        let _ = fut.await;

        // In-flight should be back to 0
        assert_eq!(service.in_flight(), 0);
    }

    #[test]
    fn test_error_display() {
        let err: AdaptiveError<&str> = AdaptiveError::LimitReached;
        assert_eq!(err.to_string(), "concurrency limit reached");

        let err: AdaptiveError<&str> = AdaptiveError::Service("test error");
        assert!(err.to_string().contains("test error"));
    }
}
