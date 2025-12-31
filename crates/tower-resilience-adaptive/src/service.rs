//! Service implementation for adaptive concurrency limiting.

use crate::ConcurrencyAlgorithm;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::Semaphore;
use tower_service::Service;

/// A service that applies adaptive concurrency limiting.
///
/// This service dynamically adjusts the number of concurrent requests based
/// on observed latency and error rates.
pub struct AdaptiveService<S, A> {
    inner: S,
    algorithm: Arc<A>,
    /// Current limit (tracked separately for dynamic adjustment)
    current_limit: Arc<AtomicUsize>,
    /// In-flight requests counter
    in_flight: Arc<AtomicUsize>,
    /// Semaphore for limiting concurrency
    semaphore: Arc<Semaphore>,
}

impl<S, A> AdaptiveService<S, A>
where
    A: ConcurrencyAlgorithm,
{
    /// Create a new adaptive service.
    pub fn new(service: S, algorithm: Arc<A>) -> Self {
        let initial_limit = algorithm.limit();
        Self {
            inner: service,
            algorithm,
            current_limit: Arc::new(AtomicUsize::new(initial_limit)),
            in_flight: Arc::new(AtomicUsize::new(0)),
            semaphore: Arc::new(Semaphore::new(initial_limit)),
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

impl<S, A> Clone for AdaptiveService<S, A>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            algorithm: Arc::clone(&self.algorithm),
            current_limit: Arc::clone(&self.current_limit),
            in_flight: Arc::clone(&self.in_flight),
            semaphore: Arc::clone(&self.semaphore),
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
        // Check if we have capacity
        let algorithm_limit = self.algorithm.limit();
        let in_flight = self.in_flight.load(Ordering::Relaxed);

        if in_flight >= algorithm_limit {
            // At capacity - wake and try again later
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        // Poll the inner service
        self.inner.poll_ready(cx).map_err(AdaptiveError::Service)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let start = Instant::now();
        self.in_flight.fetch_add(1, Ordering::Relaxed);

        let future = self.inner.call(req);

        // Adjust semaphore based on algorithm
        let algorithm_limit = self.algorithm.limit();
        let current = self.current_limit.load(Ordering::Relaxed);
        if algorithm_limit > current {
            let diff = algorithm_limit - current;
            self.semaphore.add_permits(diff);
            self.current_limit.store(algorithm_limit, Ordering::Relaxed);
        } else if algorithm_limit < current {
            self.current_limit.store(algorithm_limit, Ordering::Relaxed);
        }

        let algorithm = Arc::clone(&self.algorithm);
        let in_flight = Arc::clone(&self.in_flight);
        let semaphore = Arc::clone(&self.semaphore);
        let current_limit = Arc::clone(&self.current_limit);

        AdaptiveFuture {
            inner: Box::pin(async move {
                let result = future.await;
                let latency = start.elapsed();

                // Decrement in-flight counter
                in_flight.fetch_sub(1, Ordering::Relaxed);

                match &result {
                    Ok(_) => algorithm.on_success(latency),
                    Err(_) => algorithm.on_failure(),
                }

                // Adjust semaphore based on new algorithm limit
                let alg_limit = algorithm.limit();
                let curr = current_limit.load(Ordering::Relaxed);
                if alg_limit > curr {
                    let diff = alg_limit - curr;
                    semaphore.add_permits(diff);
                    current_limit.store(alg_limit, Ordering::Relaxed);
                } else if alg_limit < curr {
                    current_limit.store(alg_limit, Ordering::Relaxed);
                }

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

    #[tokio::test]
    async fn test_service_basic() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        let algorithm = Aimd::builder()
            .initial_limit(10)
            .latency_threshold(Duration::from_secs(1))
            .build();

        let mut service = AdaptiveService::new(service, Arc::new(algorithm));

        use tower::ServiceExt;
        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[tokio::test]
    async fn test_in_flight_tracking() {
        let service = tower::service_fn(|_req: ()| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, &str>(())
        });

        let algorithm = Aimd::builder().initial_limit(10).build();
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
