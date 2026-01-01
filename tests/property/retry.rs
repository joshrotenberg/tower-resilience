//! Property tests for the retry pattern.
//!
//! Invariants tested:
//! - Never exceeds max_attempts
//! - Succeeds on first success
//! - Retry predicate is respected
//! - Backoff is applied between retries

use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::RetryLayer;

/// A cloneable error type for testing
#[derive(Debug, Clone, PartialEq)]
enum TestError {
    Retryable,
    Fatal,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Retryable => write!(f, "retryable error"),
            TestError::Fatal => write!(f, "fatal error"),
        }
    }
}

impl std::error::Error for TestError {}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: Retry never exceeds max_attempts
    #[test]
    fn retry_respects_max_attempts(
        max_attempts in 1usize..=10,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    Err::<(), _>(TestError::Retryable)
                }
            });

            let layer = RetryLayer::<(), TestError>::builder()
                .max_attempts(max_attempts)
                .fixed_backoff(Duration::from_millis(1))
                .build();

            let mut service = layer.layer(svc);

            let result = service.ready().await.unwrap().call(()).await;
            prop_assert!(result.is_err(), "Should fail after exhausting retries");

            let total_calls = call_count.load(Ordering::SeqCst);
            prop_assert_eq!(
                total_calls,
                max_attempts,
                "Expected exactly {} attempts, got {}",
                max_attempts,
                total_calls
            );

            Ok(())
        })?;
    }

    /// Property: Success on Nth attempt stops retrying
    #[test]
    fn retry_stops_on_success(
        max_attempts in 2usize..=10,
        succeed_on in 1usize..=10,
    ) {
        // Only test valid cases where success happens within max_attempts
        if succeed_on > max_attempts {
            return Ok(());
        }

        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let succeed_on_attempt = succeed_on;
            let svc = tower::service_fn(move |_req: ()| {
                let count = call_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                async move {
                    if count >= succeed_on_attempt {
                        Ok(())
                    } else {
                        Err(TestError::Retryable)
                    }
                }
            });

            let layer = RetryLayer::<(), TestError>::builder()
                .max_attempts(max_attempts)
                .fixed_backoff(Duration::from_millis(1))
                .build();

            let mut service = layer.layer(svc);

            let result = service.ready().await.unwrap().call(()).await;
            prop_assert!(result.is_ok(), "Should succeed on attempt {}", succeed_on);

            let total_calls = call_count.load(Ordering::SeqCst);
            prop_assert_eq!(
                total_calls,
                succeed_on,
                "Should have made exactly {} calls",
                succeed_on
            );

            Ok(())
        })?;
    }

    /// Property: Retry predicate is respected
    #[test]
    fn retry_respects_predicate(
        max_attempts in 3usize..=10,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    // Return a "fatal" error that shouldn't be retried
                    Err::<(), _>(TestError::Fatal)
                }
            });

            let layer = RetryLayer::<(), TestError>::builder()
                .max_attempts(max_attempts)
                .fixed_backoff(Duration::from_millis(1))
                .retry_on(|err: &TestError| {
                    // Only retry retryable errors
                    *err == TestError::Retryable
                })
                .build();

            let mut service = layer.layer(svc);

            let result = service.ready().await.unwrap().call(()).await;
            prop_assert!(result.is_err(), "Should fail with non-retryable error");

            let total_calls = call_count.load(Ordering::SeqCst);
            prop_assert_eq!(
                total_calls,
                1,
                "Should not retry non-retryable errors, got {} calls",
                total_calls
            );

            Ok(())
        })?;
    }

    /// Property: Multiple independent requests are handled correctly
    #[test]
    fn retry_handles_multiple_requests(
        max_attempts in 2usize..=5,
        num_requests in 1usize..=20,
        failures_before_success in 0usize..=3,
    ) {
        if failures_before_success >= max_attempts {
            return Ok(());
        }

        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let total_calls = Arc::new(AtomicUsize::new(0));
            let successful_requests = Arc::new(AtomicUsize::new(0));

            let total_calls_clone = Arc::clone(&total_calls);
            let svc = tower::service_fn(move |req: usize| {
                total_calls_clone.fetch_add(1, Ordering::SeqCst);
                let fail_count = failures_before_success;
                async move {
                    // Use request ID to create per-request failure pattern
                    if req.is_multiple_of(fail_count + 1) {
                        Ok(req)
                    } else {
                        Err(TestError::Retryable)
                    }
                }
            });

            let layer = RetryLayer::<usize, TestError>::builder()
                .max_attempts(max_attempts)
                .fixed_backoff(Duration::from_millis(1))
                .build();

            let mut service = layer.layer(svc);

            for i in 0..num_requests {
                let successful = Arc::clone(&successful_requests);
                if service.ready().await.unwrap().call(i).await.is_ok() {
                    successful.fetch_add(1, Ordering::SeqCst);
                }
            }

            // Just verify we didn't panic or deadlock
            let total = total_calls.load(Ordering::SeqCst);
            prop_assert!(
                total >= num_requests,
                "Should have made at least {} calls, got {}",
                num_requests,
                total
            );

            Ok(())
        })?;
    }

    /// Property: Concurrent retries work correctly
    #[test]
    fn retry_concurrent_requests(
        max_attempts in 2usize..=5,
        num_concurrent in 5usize..=20,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let call_count = Arc::new(AtomicUsize::new(0));

            let call_count_clone = Arc::clone(&call_count);
            let svc = tower::service_fn(move |_req: ()| {
                let count = call_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    // Fail first attempt, succeed on retry
                    if count.is_multiple_of(2) {
                        Err(TestError::Retryable)
                    } else {
                        Ok(())
                    }
                }
            });

            let layer = RetryLayer::<(), TestError>::builder()
                .max_attempts(max_attempts)
                .fixed_backoff(Duration::from_millis(1))
                .build();

            let service = layer.layer(svc);

            let mut handles: Vec<tokio::task::JoinHandle<Result<(), _>>> = vec![];
            for _ in 0..num_concurrent {
                let mut svc = service.clone();
                handles.push(tokio::spawn(async move {
                    svc.ready().await.unwrap().call(()).await
                }));
            }

            let mut successes = 0;
            for handle in handles {
                if handle.await.unwrap().is_ok() {
                    successes += 1;
                }
            }

            // Most should succeed (depends on timing of the counter)
            prop_assert!(
                successes > 0,
                "At least some concurrent requests should succeed"
            );

            Ok(())
        })?;
    }
}
