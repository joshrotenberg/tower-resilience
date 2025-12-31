//! Concurrency tests for hedge pattern.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_hedge::HedgeLayer;

#[tokio::test]
async fn test_concurrent_calls_independent() {
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
    let service = layer.layer(service);

    // Spawn 10 concurrent calls
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let mut svc = service.clone();
            tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call(format!("req-{}", i))
                    .await
                    .unwrap()
            })
        })
        .collect();

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        assert_eq!(result, format!("response: req-{}", i));
    }

    // Each call should trigger only one request (fast responses)
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_concurrent_calls_with_hedging() {
    let call_count = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // All slow - hedges will fire
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(30))
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(service);

    // Spawn 5 concurrent calls
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let mut svc = service.clone();
            tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call(format!("req-{}", i))
                    .await
                    .unwrap()
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, "success");
    }

    // Each call should trigger primary + 1 hedge = 10 total
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_parallel_mode_concurrent() {
    let call_count = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let service = service_fn(move |_req: String| {
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
    let service = layer.layer(service);

    // Spawn 3 concurrent calls
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let mut svc = service.clone();
            tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call(format!("req-{}", i))
                    .await
                    .unwrap()
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, "success");
    }

    // Each call fires 3 requests (parallel mode) = 9 total
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 9);
}

#[tokio::test]
async fn test_high_concurrency_stress() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(50))
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(service);

    let sc = Arc::clone(&success_count);

    // Spawn 100 concurrent calls
    let handles: Vec<_> = (0..100)
        .map(|i| {
            let mut svc = service.clone();
            let sc = Arc::clone(&sc);
            tokio::spawn(async move {
                let result = svc.ready().await.unwrap().call(format!("req-{}", i)).await;
                if result.is_ok() {
                    sc.fetch_add(1, Ordering::SeqCst);
                }
                result
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    // All should succeed
    assert_eq!(success_count.load(Ordering::SeqCst), 100);

    // Each call should have triggered exactly 1 request (fast service)
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn test_mixed_success_failure_concurrent() {
    let call_count = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let service = service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Even requests succeed, odd fail
            if req.ends_with('0')
                || req.ends_with('2')
                || req.ends_with('4')
                || req.ends_with('6')
                || req.ends_with('8')
            {
                Ok::<_, TestError>("success".to_string())
            } else {
                Err(TestError::new("failed"))
            }
        }
    });

    let layer = HedgeLayer::<String, String, TestError>::builder()
        .no_delay()
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(service);

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let mut svc = service.clone();
            tokio::spawn(async move {
                let result = svc.ready().await.unwrap().call(format!("req-{}", i)).await;
                (i, result.is_ok())
            })
        })
        .collect();

    let mut successes = 0;
    let mut failures = 0;
    for handle in handles {
        let (i, success) = handle.await.unwrap();
        if success {
            // Even numbers should succeed (0, 2, 4, 6, 8)
            assert!(i % 2 == 0, "expected even {} to succeed", i);
            successes += 1;
        } else {
            // Odd numbers should fail (1, 3, 5, 7, 9)
            assert!(i % 2 == 1, "expected odd {} to fail", i);
            failures += 1;
        }
    }

    assert_eq!(successes, 5);
    assert_eq!(failures, 5);
}

#[tokio::test]
async fn test_cancellation_on_first_success() {
    // This test verifies that when one request succeeds, the others are
    // effectively cancelled (their results are ignored).
    let completed_count = Arc::new(AtomicUsize::new(0));
    let started_count = Arc::new(AtomicUsize::new(0));

    let sc = Arc::clone(&started_count);
    let cc = Arc::clone(&completed_count);
    let service = service_fn(move |_req: String| {
        let sc = Arc::clone(&sc);
        let cc = Arc::clone(&cc);
        async move {
            let attempt = sc.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                // Primary is slow
                tokio::time::sleep(Duration::from_millis(200)).await;
            } else {
                // Hedge is fast
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(30))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(service);

    let start = std::time::Instant::now();
    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    let elapsed = start.elapsed();

    // Should complete quickly (hedge wins)
    assert!(
        elapsed < Duration::from_millis(100),
        "elapsed: {:?}",
        elapsed
    );

    // Both should have started
    assert_eq!(started_count.load(Ordering::SeqCst), 2);

    // Give time for primary to complete
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Both should eventually complete (tasks run to completion)
    assert_eq!(completed_count.load(Ordering::SeqCst), 2);
}
