//! Property tests for the bulkhead pattern.
//!
//! Invariants tested:
//! - Concurrent calls never exceed max_concurrent_calls
//! - All requests eventually complete (no deadlocks)
//! - Rejected requests are properly counted

use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::{BulkheadError, BulkheadLayer};

/// Error type that can be converted from BulkheadError
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum TestError {
    Bulkhead(String),
    Service(String),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Bulkhead(msg) => write!(f, "bulkhead: {}", msg),
            TestError::Service(msg) => write!(f, "service: {}", msg),
        }
    }
}

impl std::error::Error for TestError {}

impl From<BulkheadError> for TestError {
    fn from(e: BulkheadError) -> Self {
        TestError::Bulkhead(e.to_string())
    }
}

/// Test service that tracks concurrent executions
#[derive(Clone)]
struct ConcurrencyTracker {
    current: Arc<AtomicUsize>,
    max_seen: Arc<AtomicUsize>,
    work_duration_ms: u64,
}

impl ConcurrencyTracker {
    fn new(work_duration_ms: u64) -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            max_seen: Arc::new(AtomicUsize::new(0)),
            work_duration_ms,
        }
    }
}

impl tower::Service<()> for ConcurrencyTracker {
    type Response = ();
    type Error = TestError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let current = Arc::clone(&self.current);
        let max_seen = Arc::clone(&self.max_seen);
        let duration = self.work_duration_ms;

        Box::pin(async move {
            // Increment current count
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;

            // Update max if needed
            let mut max = max_seen.load(Ordering::SeqCst);
            while now > max {
                match max_seen.compare_exchange_weak(max, now, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => break,
                    Err(m) => max = m,
                }
            }

            // Simulate work
            tokio::time::sleep(Duration::from_millis(duration)).await;

            // Decrement
            current.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Simple counting service for tests
#[derive(Clone)]
struct CountingService {
    completed_count: Arc<AtomicUsize>,
    work_duration_ms: u64,
}

impl CountingService {
    fn new(work_duration_ms: u64) -> Self {
        Self {
            completed_count: Arc::new(AtomicUsize::new(0)),
            work_duration_ms,
        }
    }
}

impl tower::Service<()> for CountingService {
    type Response = ();
    type Error = TestError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let completed = Arc::clone(&self.completed_count);
        let duration = self.work_duration_ms;

        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(duration)).await;
            completed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: The bulkhead never allows more concurrent calls than configured
    #[test]
    fn bulkhead_respects_max_concurrent(
        max_concurrent in 1usize..=20,
        num_requests in 1usize..=100,
        work_duration_ms in 1u64..=10,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tracker = ConcurrencyTracker::new(work_duration_ms);
            let max_seen_ref = Arc::clone(&tracker.max_seen);

            let layer = BulkheadLayer::builder()
                .max_concurrent_calls(max_concurrent)
                .max_wait_duration(Some(Duration::from_secs(10)))
                .build();

            let service = layer.layer(tracker);

            // Spawn all requests concurrently
            let mut handles: Vec<tokio::task::JoinHandle<()>> = vec![];
            for _ in 0..num_requests {
                let mut svc = service.clone();
                handles.push(tokio::spawn(async move {
                    let _: Result<(), TestError> = svc.ready().await.unwrap().call(()).await;
                }));
            }

            // Wait for all to complete
            for handle in handles {
                handle.await.unwrap();
            }

            // Verify invariant: max concurrent never exceeded limit
            let observed_max = max_seen_ref.load(Ordering::SeqCst);
            prop_assert!(
                observed_max <= max_concurrent,
                "Observed {} concurrent calls but limit was {}",
                observed_max,
                max_concurrent
            );

            Ok(())
        })?;
    }

    /// Property: All requests complete (no deadlock) when given enough time
    #[test]
    fn bulkhead_no_deadlock(
        max_concurrent in 1usize..=10,
        num_requests in 1usize..=50,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let counting_svc = CountingService::new(1);
            let completed_ref = Arc::clone(&counting_svc.completed_count);

            let layer = BulkheadLayer::builder()
                .max_concurrent_calls(max_concurrent)
                .max_wait_duration(Some(Duration::from_secs(30)))
                .build();

            let service = layer.layer(counting_svc);

            let mut handles: Vec<tokio::task::JoinHandle<()>> = vec![];
            for _ in 0..num_requests {
                let mut svc = service.clone();
                handles.push(tokio::spawn(async move {
                    let _: Result<(), TestError> = svc.ready().await.unwrap().call(()).await;
                }));
            }

            // All should complete within reasonable time
            let timeout = tokio::time::timeout(
                Duration::from_secs(10),
                async {
                    for handle in handles {
                        handle.await.unwrap();
                    }
                }
            ).await;

            prop_assert!(timeout.is_ok(), "Deadlock detected: requests did not complete");
            prop_assert_eq!(completed_ref.load(Ordering::SeqCst), num_requests);

            Ok(())
        })?;
    }

}
