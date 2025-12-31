//! Integration tests for hedge basic functionality.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_hedge::{HedgeError, HedgeLayer};

#[tokio::test]
async fn test_success_no_hedge_needed() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_string())
        .await
        .unwrap();

    assert_eq!(response, "response: hello");

    // Give time for any hedges to potentially fire
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Should only have called once since primary was fast
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_slow_primary_triggers_hedge() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                // Primary is slow
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            // Hedges respond quickly
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(50))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(service);

    let start = std::time::Instant::now();
    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(response, "response");

    // Should complete faster than 200ms because hedge succeeded
    assert!(
        elapsed < Duration::from_millis(150),
        "elapsed: {:?}",
        elapsed
    );

    // Both primary and hedge should have been called
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_all_attempts_fail() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::new("failed"))
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

    assert!(matches!(result, Err(HedgeError::AllAttemptsFailed(_))));

    // All 3 attempts should have been made
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_multiple_sequential_calls() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
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

    // Make multiple sequential calls
    for i in 0..5 {
        let response = service
            .ready()
            .await
            .unwrap()
            .call(format!("req-{}", i))
            .await
            .unwrap();
        assert_eq!(response, "success");
    }

    // Each call should only trigger primary (fast response)
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_service_cloning_preserves_config() {
    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(50))
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(service);

    // Clone the service
    let mut clone1 = service.clone();
    let mut clone2 = service.clone();

    // Both clones should work identically
    let r1 = clone1
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    let r2 = clone2
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    assert_eq!(r1, "success");
    assert_eq!(r2, "success");
}

#[tokio::test]
async fn test_named_hedge() {
    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .name("my-hedge")
        .delay(Duration::from_millis(50))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "success");
}

#[tokio::test]
async fn test_max_attempts_one_no_hedges() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Slow response, but no hedge should fire
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(10))
        .max_hedged_attempts(1) // Only primary, no hedges
        .build();
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "success");

    tokio::time::sleep(Duration::from_millis(20)).await;
    // Should only have called once
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_error_preserved_in_all_failed() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::new("original error message"))
    });

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

    match result {
        Err(HedgeError::AllAttemptsFailed(e)) => {
            assert_eq!(e.message, "original error message");
        }
        _ => panic!("expected AllAttemptsFailed error"),
    }
}
