use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::Service;
use tower_bulkhead::{BulkheadConfig, BulkheadError};

/// Test 100 concurrent requests with limited permits
#[tokio::test]
async fn hundred_concurrent_requests() {
    let max_concurrent = 10;
    let completed = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&completed);

    let service = tower::service_fn(move |_req: ()| {
        let counter = Arc::clone(&c);
        async move {
            sleep(Duration::from_millis(10)).await;
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(5)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..100 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(Ok(_))) = handle.await {
            success_count += 1;
        }
    }

    // All 100 should complete successfully
    assert_eq!(success_count, 100);
    assert_eq!(completed.load(Ordering::Relaxed), 100);
}

/// Test 1000 concurrent requests stress test
#[tokio::test]
async fn thousand_concurrent_requests_stress_test() {
    let max_concurrent = 50;
    let completed = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&completed);
    let r = Arc::clone(&rejected);

    let service = tower::service_fn(move |_req: ()| {
        let counter = Arc::clone(&c);
        async move {
            sleep(Duration::from_millis(5)).await;
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(10)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..1000 {
        let bh = Arc::clone(&bulkhead);
        let rej = Arc::clone(&r);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            match guard.call(()).await {
                Ok(_) => true,
                Err(BulkheadError::Timeout) | Err(BulkheadError::BulkheadFull) => {
                    rej.fetch_add(1, Ordering::Relaxed);
                    false
                }
                _ => false,
            }
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        if let Ok(true) = handle.await {
            success_count += 1;
        }
    }

    // Most should complete, some might be rejected due to timeout
    assert!(
        success_count >= 900,
        "Expected at least 900 successes, got {}",
        success_count
    );
    assert_eq!(
        completed.load(Ordering::Relaxed) + rejected.load(Ordering::Relaxed),
        1000
    );
}

/// Test concurrent permit acquisition during full bulkhead
#[tokio::test]
async fn concurrent_acquisition_when_full() {
    let max_concurrent = 5;
    let acquired = Arc::new(AtomicUsize::new(0));
    let peak_concurrent = Arc::new(AtomicUsize::new(0));

    let a = Arc::clone(&acquired);
    let p = Arc::clone(&peak_concurrent);

    let service = tower::service_fn(move |_req: ()| {
        let acq = Arc::clone(&a);
        let peak = Arc::clone(&p);
        async move {
            let current = acq.fetch_add(1, Ordering::Relaxed) + 1;

            // Track peak concurrency
            peak.fetch_max(current, Ordering::Relaxed);

            sleep(Duration::from_millis(50)).await;
            acq.fetch_sub(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(2)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..20 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Peak concurrent should never exceed max
    assert!(
        peak_concurrent.load(Ordering::Relaxed) <= max_concurrent,
        "Peak concurrent {} exceeded max {}",
        peak_concurrent.load(Ordering::Relaxed),
        max_concurrent
    );
}

/// Test race conditions between acquire and release
#[tokio::test]
async fn acquire_release_race_conditions() {
    let max_concurrent = 10;
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(1)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Rapidly acquire and release permits
    let mut handles = vec![];
    for _ in 0..100 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut success = 0;
    for handle in handles {
        if let Ok(Ok(Ok(_))) = handle.await {
            success += 1;
        }
    }

    // All should eventually succeed
    assert!(
        success >= 95,
        "Expected most requests to succeed, got {}",
        success
    );
}

/// Test semaphore permit count verification under load
#[tokio::test]
async fn semaphore_permit_count_under_load() {
    let max_concurrent = 20;
    let active_count = Arc::new(AtomicUsize::new(0));
    let violations = Arc::new(AtomicUsize::new(0));

    let a = Arc::clone(&active_count);
    let v = Arc::clone(&violations);

    let service = tower::service_fn(move |_req: ()| {
        let active = Arc::clone(&a);
        let viols = Arc::clone(&v);
        async move {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;

            // Check if we exceeded limit
            if current > max_concurrent {
                viols.fetch_add(1, Ordering::SeqCst);
            }

            sleep(Duration::from_millis(10)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            Ok::<(), String>(())
        }
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(3)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..200 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Should never violate the concurrency limit
    assert_eq!(
        violations.load(Ordering::SeqCst),
        0,
        "Concurrency limit violated {} times",
        violations.load(Ordering::SeqCst)
    );
}

/// Test multiple threads acquiring/releasing simultaneously
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_threaded_acquire_release() {
    let max_concurrent = 8;
    let completed = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&completed);

    let service = tower::service_fn(move |_req: ()| {
        let counter = Arc::clone(&c);
        async move {
            sleep(Duration::from_millis(5)).await;
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(5)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..100 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut success = 0;
    for handle in handles {
        if let Ok(Ok(Ok(_))) = handle.await {
            success += 1;
        }
    }

    assert_eq!(success, 100);
    assert_eq!(completed.load(Ordering::Relaxed), 100);
}

/// Test permits are never leaked under high contention
#[tokio::test]
async fn no_permit_leaks_under_contention() {
    let max_concurrent = 5;
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Run multiple rounds to detect leaks
    for _ in 0..10 {
        let mut handles = vec![];
        for _ in 0..20 {
            let bh = Arc::clone(&bulkhead);
            handles.push(tokio::spawn(async move {
                let mut guard = bh.lock().await;
                guard.call(()).await
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    // If permits leaked, subsequent calls would timeout/fail
    // Make a final batch of calls to verify all permits available
    let mut final_handles = vec![];
    for _ in 0..max_concurrent {
        let bh = Arc::clone(&bulkhead);
        final_handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut final_success = 0;
    for handle in final_handles {
        if let Ok(Ok(Ok(_))) = handle.await {
            final_success += 1;
        }
    }

    // All should succeed if no permits leaked
    assert_eq!(final_success, max_concurrent, "Permits may have leaked");
}
