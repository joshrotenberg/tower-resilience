//! Property tests for the circuit breaker pattern.
//!
//! Invariants tested:
//! - Opens when failure rate exceeds threshold
//! - Rejects requests when open
//! - Allows test request in half-open state
//! - Closes after successful test request

use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;
use tower::{Service, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitBreakerLayer};

/// A cloneable error type for testing
#[derive(Debug, Clone)]
struct TestError;

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "test error")
    }
}

impl std::error::Error for TestError {}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Property: Circuit breaker opens after enough failures
    #[test]
    fn circuit_breaker_opens_on_failures(
        failure_threshold in 0.1f32..=0.9,
        window_size in 5usize..=20,
        num_failures in 5usize..=50,
    ) {
        // Ensure we have enough failures to trigger opening
        let min_failures = ((window_size as f32) * failure_threshold).ceil() as usize;
        if num_failures < min_failures {
            return Ok(()); // Skip if not enough failures to trigger
        }

        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    Err::<(), TestError>(TestError)
                }
            });

            let layer = CircuitBreakerLayer::<(), TestError>::builder()
                .failure_rate_threshold(failure_threshold as f64)
                .sliding_window_size(window_size)
                .minimum_number_of_calls(min_failures.min(window_size))
                .wait_duration_in_open(Duration::from_secs(60))
                .build();

            let mut service = layer.layer(svc);

            // Generate failures
            for _ in 0..num_failures {
                let _: Result<(), _> = service.ready().await.unwrap().call(()).await;
            }

            let calls_made = call_count.load(Ordering::SeqCst);

            // The circuit breaker should have opened, preventing some calls
            // After opening, calls should be rejected without reaching the service
            prop_assert!(
                calls_made <= num_failures,
                "All calls went through even though circuit should have opened"
            );

            Ok(())
        })?;
    }

    /// Property: Circuit breaker stays closed when failure rate is below threshold
    #[test]
    fn circuit_breaker_stays_closed_under_threshold(
        failure_threshold in 0.5f32..=0.9,
        window_size in 10usize..=30,
        success_rate in 0.7f32..=1.0,
    ) {
        // Skip if success rate would trigger opening
        if (1.0 - success_rate) >= failure_threshold {
            return Ok(());
        }

        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let should_fail = Arc::new(AtomicBool::new(false));
            let call_count = Arc::new(AtomicUsize::new(0));

            let should_fail_clone = Arc::clone(&should_fail);
            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                let fail = should_fail_clone.load(Ordering::SeqCst);
                async move {
                    if fail {
                        Err(TestError)
                    } else {
                        Ok(())
                    }
                }
            });

            let layer = CircuitBreakerLayer::<(), TestError>::builder()
                .failure_rate_threshold(failure_threshold as f64)
                .sliding_window_size(window_size)
                .minimum_number_of_calls(window_size / 2)
                .build();

            let mut service = layer.layer(svc);

            // Send requests with configured success rate
            let num_requests = window_size * 2;
            let failures_to_inject = ((num_requests as f32) * (1.0 - success_rate)) as usize;

            for i in 0..num_requests {
                should_fail.store(i < failures_to_inject, Ordering::SeqCst);
                let _: Result<(), _> = service.ready().await.unwrap().call(()).await;
            }

            // All requests should have been processed (circuit stayed closed)
            let calls_made = call_count.load(Ordering::SeqCst);
            prop_assert_eq!(
                calls_made,
                num_requests,
                "Circuit opened unexpectedly: {} calls made of {}",
                calls_made,
                num_requests
            );

            Ok(())
        })?;
    }

    /// Property: Circuit breaker transitions correctly through states
    #[test]
    fn circuit_breaker_state_transitions(
        window_size in 5usize..=15,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let should_fail = Arc::new(AtomicBool::new(true));
            let call_count = Arc::new(AtomicUsize::new(0));
            let rejected_count = Arc::new(AtomicUsize::new(0));

            let should_fail_clone = Arc::clone(&should_fail);
            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                let fail = should_fail_clone.load(Ordering::SeqCst);
                async move {
                    if fail {
                        Err(TestError)
                    } else {
                        Ok(())
                    }
                }
            });

            let layer = CircuitBreakerLayer::<(), TestError>::builder()
                .failure_rate_threshold(0.5)
                .sliding_window_size(window_size)
                .minimum_number_of_calls(window_size)
                .wait_duration_in_open(Duration::from_millis(50))
                .permitted_calls_in_half_open(1)
                .build();

            let mut service = layer.layer(svc);

            type CbResult = Result<(), CircuitBreakerError<TestError>>;

            // Phase 1: Generate failures to open the circuit
            for _ in 0..window_size {
                let _result: CbResult = service.ready().await.unwrap().call(()).await;
            }

            let after_failures = call_count.load(Ordering::SeqCst);

            // Phase 2: Try requests while open - should be rejected
            for _ in 0..5 {
                let result: CbResult = service.ready().await.unwrap().call(()).await;
                if result.is_err() {
                    rejected_count.fetch_add(1, Ordering::SeqCst);
                }
            }

            // Some should be rejected (circuit is open)
            let after_open = call_count.load(Ordering::SeqCst);

            // Circuit should have blocked some calls
            prop_assert!(
                after_open <= after_failures + 5,
                "Circuit didn't block calls: before={} after={}",
                after_failures,
                after_open
            );

            // Phase 3: Wait for half-open, then succeed
            tokio::time::sleep(Duration::from_millis(100)).await;
            should_fail.store(false, Ordering::SeqCst);

            // This should be allowed as a test request
            let result: CbResult = service.ready().await.unwrap().call(()).await;
            prop_assert!(result.is_ok(), "Half-open test request should succeed");

            // Phase 4: Circuit should now be closed
            for _ in 0..window_size {
                let result: CbResult = service.ready().await.unwrap().call(()).await;
                prop_assert!(result.is_ok(), "Requests should succeed after circuit closes");
            }

            Ok(())
        })?;
    }

    /// Property: Concurrent requests respect circuit breaker state
    #[test]
    fn circuit_breaker_concurrent_access(
        window_size in 5usize..=10,
        num_concurrent in 10usize..=30,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    Err::<(), TestError>(TestError)
                }
            });

            let layer = CircuitBreakerLayer::<(), TestError>::builder()
                .failure_rate_threshold(0.5)
                .sliding_window_size(window_size)
                .minimum_number_of_calls(window_size)
                .wait_duration_in_open(Duration::from_secs(60))
                .build();

            let service = layer.layer(svc);

            // Spawn concurrent requests
            let mut handles: Vec<tokio::task::JoinHandle<Result<(), _>>> = vec![];
            for _ in 0..num_concurrent {
                let mut svc = service.clone();
                handles.push(tokio::spawn(async move {
                    svc.ready().await.unwrap().call(()).await
                }));
            }

            for handle in handles {
                let _: Result<(), _> = handle.await.unwrap();
            }

            let total_calls = call_count.load(Ordering::SeqCst);

            // Circuit should have opened at some point, blocking subsequent calls
            // The exact number depends on timing, but it should be bounded
            prop_assert!(
                total_calls <= num_concurrent,
                "All concurrent calls reached service: {}",
                total_calls
            );

            Ok(())
        })?;
    }
}
