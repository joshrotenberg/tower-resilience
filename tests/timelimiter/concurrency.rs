//! Concurrency tests for tower-timelimiter.
//!
//! Tests that verify timeout behavior under concurrent load.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterConfig;

#[tokio::test]
async fn concurrent_calls_with_same_timeout() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer();

    let svc = service_fn(|req: u32| async move {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>(req * 2)
    });

    let service = layer.layer(svc);

    // Spawn 100 concurrent calls
    let mut handles = vec![];
    for i in 0..100 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move { svc.ready().await.unwrap().call(i).await });
        handles.push(handle);
    }

    // Collect results
    let mut success_count = 0;
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Call {} failed: {:?}", i, result);
        assert_eq!(result.unwrap(), (i as u32) * 2);
        success_count += 1;
    }

    assert_eq!(success_count, 100);
}

#[tokio::test]
async fn concurrent_calls_with_different_timeouts() {
    let timeouts = [
        Duration::from_millis(30),
        Duration::from_millis(50),
        Duration::from_millis(100),
        Duration::from_millis(200),
    ];

    let mut handles = vec![];

    for (i, timeout) in timeouts.iter().enumerate() {
        for j in 0..25 {
            let layer = TimeLimiterConfig::builder()
                .timeout_duration(*timeout)
                .build()
                .layer();

            let svc = service_fn(|req: u32| async move {
                sleep(Duration::from_millis(10)).await;
                Ok::<_, TestError>(req)
            });

            let mut service = layer.layer(svc);
            let req_id = (i * 25 + j) as u32;
            let handle =
                tokio::spawn(async move { service.ready().await.unwrap().call(req_id).await });
            handles.push(handle);
        }
    }

    // All should succeed since they all complete in 10ms
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.unwrap();
        if result.is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 100);
}

#[tokio::test]
async fn some_timeout_some_succeed_concurrently() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer();

    let svc = service_fn(|req: u32| async move {
        if req.is_multiple_of(2) {
            // Even requests complete quickly
            sleep(Duration::from_millis(10)).await;
        } else {
            // Odd requests timeout
            sleep(Duration::from_millis(100)).await;
        }
        Ok::<_, TestError>(req)
    });

    let service = layer.layer(svc);

    let mut handles = vec![];
    for i in 0..100 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move { svc.ready().await.unwrap().call(i).await });
        handles.push(handle);
    }

    let mut success_count = 0;
    let mut timeout_count = 0;

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        if i % 2 == 0 {
            assert!(result.is_ok(), "Even request {} should succeed", i);
            success_count += 1;
        } else {
            assert!(result.is_err(), "Odd request {} should timeout", i);
            assert!(result.unwrap_err().is_timeout());
            timeout_count += 1;
        }
    }

    assert_eq!(success_count, 50);
    assert_eq!(timeout_count, 50);
}

#[tokio::test]
async fn all_timeout_simultaneously() {
    let timeout_count = Arc::new(AtomicUsize::new(0));
    let tc_clone = Arc::clone(&timeout_count);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .on_timeout(move || {
            tc_clone.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .layer();

    let svc = service_fn(|_req: u32| async move {
        sleep(Duration::from_millis(200)).await;
        Ok::<_, TestError>("should not complete")
    });

    let service = layer.layer(svc);

    let mut handles = vec![];
    for i in 0..100 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move { svc.ready().await.unwrap().call(i).await });
        handles.push(handle);
    }

    // All should timeout
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().is_timeout());
    }

    assert_eq!(timeout_count.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn no_resource_leaks_under_load() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer();

    // Run multiple rounds to stress test
    for round in 0..10 {
        let svc = service_fn(move |req: u32| async move {
            if req.is_multiple_of(2) {
                sleep(Duration::from_millis(10)).await;
                Ok::<_, TestError>(req)
            } else {
                sleep(Duration::from_millis(100)).await;
                Ok(req)
            }
        });

        let service = layer.clone().layer(svc);

        let mut handles = vec![];
        for i in 0..50 {
            let mut svc = service.clone();
            let handle = tokio::spawn(async move { svc.ready().await.unwrap().call(i).await });
            handles.push(handle);
        }

        // Wait for all to complete
        for handle in handles {
            let _ = handle.await.unwrap();
        }

        // Small delay between rounds
        if round < 9 {
            sleep(Duration::from_millis(10)).await;
        }
    }

    // If we got here without panicking or running out of memory, we're good
    // In a real scenario, you'd use tools like valgrind or address sanitizer
}

#[tokio::test]
async fn independent_timeout_timers() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer();

    // Start services at different times to verify timers are independent
    let svc1 = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("should timeout")
    });
    let mut service1 = layer.clone().layer(svc1);

    // Start first call
    let handle1 = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let result = service1.ready().await.unwrap().call(()).await;
        (result, start.elapsed())
    });

    // Wait a bit before starting second call
    sleep(Duration::from_millis(25)).await;

    let svc2 = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("should timeout")
    });
    let mut service2 = layer.layer(svc2);

    let handle2 = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let result = service2.ready().await.unwrap().call(()).await;
        (result, start.elapsed())
    });

    let (result1, elapsed1) = handle1.await.unwrap();
    let (result2, elapsed2) = handle2.await.unwrap();

    // Both should timeout
    assert!(result1.is_err());
    assert!(result1.unwrap_err().is_timeout());
    assert!(result2.is_err());
    assert!(result2.unwrap_err().is_timeout());

    // First call should timeout around 50ms
    // Windows has ~15.6ms timer resolution, so use generous tolerance
    assert!(
        elapsed1.as_millis() >= 30 && elapsed1.as_millis() <= 90,
        "Expected timeout ~50ms, got {}ms",
        elapsed1.as_millis()
    );
    // Second call should also timeout around 50ms (not 75ms)
    assert!(
        elapsed2.as_millis() >= 30 && elapsed2.as_millis() <= 90,
        "Expected timeout ~50ms, got {}ms",
        elapsed2.as_millis()
    );
}
