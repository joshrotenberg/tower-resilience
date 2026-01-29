//! Order verification tests.
//!
//! These tests go beyond compile-time verification to ensure that
//! layer ordering actually affects runtime behavior as documented.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tower::{Layer, Service};
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::{TimeLimiterError, TimeLimiterLayer};

use super::external_api::{ApiError, ApiRequest, ApiResponse};

/// Verify total timeout bounds all retry attempts.
///
/// When timeout is outermost, it limits the total time for all retries,
/// not just individual attempts. This test proves the ordering matters.
#[tokio::test]
async fn total_timeout_bounds_all_retries() {
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    // Service that always fails after 500ms
    let slow_failing_service = tower::service_fn(move |_req: ApiRequest| {
        let count = call_count_clone.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(500)).await;
            Err::<ApiResponse, _>(ApiError("always fails".into()))
        }
    });

    let retry = RetryLayer::<ApiRequest, ApiError>::builder()
        .max_attempts(10) // Would take 5s+ without total timeout
        .build();

    let total_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1)) // Only allows ~2 attempts
        .build();

    // Correct order: timeout outermost (applied last in manual composition)
    let with_retry = retry.layer(slow_failing_service);
    let mut service = total_timeout.layer(with_retry);

    let start = std::time::Instant::now();
    let result = service.call(ApiRequest::new("test")).await;
    let elapsed = start.elapsed();

    assert!(result.is_err()); // Should timeout
    assert!(
        elapsed < Duration::from_secs(3),
        "Should not wait for all retries, elapsed: {:?}",
        elapsed
    );
    assert!(
        call_count.load(Ordering::SeqCst) <= 3,
        "Should have limited attempts due to timeout, got: {}",
        call_count.load(Ordering::SeqCst)
    );
}

/// Verify that without total timeout, retries continue indefinitely.
///
/// This is the inverse of the above test - proves that timeout IS needed
/// to bound retry behavior.
#[tokio::test]
async fn without_timeout_retries_continue() {
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_clone = call_count.clone();

    // Service that always fails quickly
    let fast_failing_service = tower::service_fn(move |_req: ApiRequest| {
        let count = call_count_clone.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<ApiResponse, _>(ApiError("always fails".into()))
        }
    });

    let retry = RetryLayer::<ApiRequest, ApiError>::builder()
        .max_attempts(5)
        .build();

    let mut service = retry.layer(fast_failing_service);

    let result = service.call(ApiRequest::new("test")).await;

    assert!(result.is_err()); // Should exhaust retries
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        5,
        "Should have made all 5 attempts"
    );
}

/// Verify retry recovers from transient failures.
///
/// This test shows retry working correctly - failing twice, then succeeding.
#[tokio::test]
async fn retry_recovers_from_transient_failures() {
    let attempt_count = Arc::new(AtomicU32::new(0));
    let attempt_clone = attempt_count.clone();

    // Service that fails twice then succeeds
    let flaky_service = tower::service_fn(move |_req: ApiRequest| {
        let attempts = attempt_clone.clone();
        async move {
            let current = attempts.fetch_add(1, Ordering::SeqCst);
            if current < 2 {
                Err::<ApiResponse, _>(ApiError("transient".into()))
            } else {
                Ok(ApiResponse::new("success"))
            }
        }
    });

    let retry = RetryLayer::<ApiRequest, ApiError>::builder()
        .max_attempts(5)
        .build();

    let mut service = retry.layer(flaky_service);

    let result = service.call(ApiRequest::new("test")).await;

    assert!(result.is_ok(), "Should succeed after retries");
    assert_eq!(
        attempt_count.load(Ordering::SeqCst),
        3,
        "Should have made 3 attempts (2 failures + 1 success)"
    );
}

/// Verify timeout layer terminates slow service.
///
/// Basic test showing timeout works independently.
#[tokio::test]
async fn timeout_terminates_slow_service() {
    let slow_service = tower::service_fn(|_req: ApiRequest| async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok::<_, ApiError>(ApiResponse::new("too slow"))
    });

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let mut service = timeout.layer(slow_service);

    let start = std::time::Instant::now();
    let result = service.call(ApiRequest::new("test")).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Should timeout");
    assert!(
        elapsed < Duration::from_millis(200),
        "Should have timed out quickly"
    );
}

/// Verify timeout produces a recognizable error type.
///
/// When timeout wraps a service, errors become TimeLimiterError::Timeout.
#[tokio::test]
async fn timeout_error_is_identifiable() {
    let slow_service = tower::service_fn(|_req: ApiRequest| async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok::<_, ApiError>(ApiResponse::new("never reached"))
    });

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let mut service = timeout.layer(slow_service);

    let result: Result<ApiResponse, TimeLimiterError<ApiError>> =
        service.call(ApiRequest::new("test")).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, TimeLimiterError::Timeout),
        "Error should be a timeout, got: {:?}",
        err
    );
}
