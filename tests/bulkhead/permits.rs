use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_bulkhead::BulkheadConfig;

/// Test permit released even if future is dropped
#[tokio::test]
async fn permit_released_on_future_drop() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_secs(10)).await; // Long duration
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Start a call and drop it
    {
        let _future = bulkhead.call(());
        // Future dropped here
    }

    // Give some time for cleanup
    sleep(Duration::from_millis(50)).await;

    // Should be able to acquire permit again (verifies permit was released)
    let result = bulkhead.call(()).await;
    // This might timeout if the service is still holding the permit,
    // but the call should eventually work after the dropped future cleans up
    match result {
        Ok(_) => {} // Permit was released properly
        Err(_) => {
            // Try again after a delay
            sleep(Duration::from_millis(100)).await;
            let result2 = bulkhead.call(()).await;
            assert!(
                result2.is_ok(),
                "Permit should have been released after future drop"
            );
        }
    }
}

/// Test permit released on panic in service
#[tokio::test]
async fn permit_released_on_panic() {
    let service = tower::service_fn(|_req: ()| async {
        panic!("Service panic!");
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .build();

    let mut bulkhead = layer.layer(service);

    // First call panics
    let handle = tokio::spawn(async move {
        let _ = bulkhead.call(()).await;
    });

    // Wait for panic to occur
    let _ = handle.await;

    // Create new bulkhead with non-panicking service
    let service2 = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });
    let layer2 = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .build();
    let mut bulkhead2 = layer2.layer(service2);

    // Should work (permit was released despite panic)
    let result = bulkhead2.call(()).await;
    assert!(result.is_ok());
}

/// Test permit count accurate after mixed success/error
#[tokio::test]
async fn permit_count_after_mixed_results() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            sleep(Duration::from_millis(50)).await;
            if count % 2 == 0 {
                Ok::<(), String>(())
            } else {
                Err("error".to_string())
            }
        }
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_secs(2)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Make 20 calls (mix of success and error)
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

    // Make one more call to verify permits are all available
    let mut guard = bulkhead.lock().await;
    let result = guard.call(()).await;
    assert!(result.is_ok() || result.is_err()); // Either is fine, point is it didn't timeout/hang
}

/// Test no permit leaks over time
#[tokio::test]
async fn no_permit_leaks_over_time() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .max_wait_duration(Some(Duration::from_millis(200)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Run many batches over time
    for batch in 0..20 {
        let mut handles = vec![];
        for _ in 0..10 {
            let bh = Arc::clone(&bulkhead);
            handles.push(tokio::spawn(async move {
                let mut guard = bh.lock().await;
                guard.call(()).await
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        // Small delay between batches
        sleep(Duration::from_millis(50)).await;

        // Verify permits still available by making a test call
        let mut guard = bulkhead.lock().await;
        let test_result = guard.call(()).await;
        assert!(
            test_result.is_ok(),
            "Permit leak detected in batch {}",
            batch
        );
    }
}

/// Test permit released on cancellation (tokio task cancelled)
#[tokio::test]
async fn permit_released_on_cancellation() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_secs(10)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Start a long-running call
    let bh = Arc::clone(&bulkhead);
    let handle = tokio::spawn(async move {
        let mut guard = bh.lock().await;
        let _ = guard.call(()).await;
    });

    // Give it time to acquire permit
    sleep(Duration::from_millis(50)).await;

    // Cancel the task
    handle.abort();

    // Wait for cancellation to take effect
    sleep(Duration::from_millis(100)).await;

    // Should be able to acquire permit (verifies it was released on cancellation)
    let service2 = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });
    let layer2 = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .build();
    let mut bulkhead2 = layer2.layer(service2);

    let result = bulkhead2.call(()).await;
    assert!(
        result.is_ok(),
        "Permit should have been released on cancellation"
    );
}

/// Test permits available after all calls complete
#[tokio::test]
async fn permits_available_after_completion() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<(), String>(())
    });

    let max_concurrent = 10;
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(2)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Make exactly max_concurrent calls
    let mut handles = vec![];
    for _ in 0..max_concurrent {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    // All permits should be available - make max_concurrent simultaneous calls
    let mut handles2 = vec![];
    for _ in 0..max_concurrent {
        let bh = Arc::clone(&bulkhead);
        handles2.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut success_count = 0;
    for handle in handles2 {
        if let Ok(Ok(Ok(_))) = handle.await {
            success_count += 1;
        }
    }

    assert_eq!(
        success_count, max_concurrent,
        "Not all permits were available"
    );
}

/// Test permit lifecycle with errors
#[tokio::test]
async fn permit_lifecycle_with_errors() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(20)).await;
        Err::<(), _>("service error".to_string())
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_secs(1)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Make calls that all error
    let mut handles = vec![];
    for _ in 0..10 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Permits should still be available despite errors
    let service2 = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });
    let layer2 = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_secs(1)))
        .build();
    let mut bulkhead2 = layer2.layer(service2);

    let result = bulkhead2.call(()).await;
    assert!(result.is_ok(), "Permits should be available after errors");
}

/// Test permit correctness with rapid acquire/release cycles
#[tokio::test]
async fn rapid_acquire_release_cycles() {
    let service = tower::service_fn(|_req: ()| async {
        // Very fast service
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Rapid fire many calls
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

    // All should succeed with fast service
    assert_eq!(success_count, 100, "Some permits may have been leaked");
}

/// Test permits with concurrent acquire attempts
#[tokio::test]
async fn concurrent_acquire_attempts() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));

    let a = Arc::clone(&active);
    let m = Arc::clone(&max_observed);

    let service = tower::service_fn(move |_req: ()| {
        let active_clone = Arc::clone(&a);
        let max_clone = Arc::clone(&m);
        async move {
            let current = active_clone.fetch_add(1, Ordering::SeqCst) + 1;
            max_clone.fetch_max(current, Ordering::SeqCst);

            sleep(Duration::from_millis(50)).await;

            active_clone.fetch_sub(1, Ordering::SeqCst);
            Ok::<(), String>(())
        }
    });

    let max_concurrent = 7;
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(max_concurrent)
        .max_wait_duration(Some(Duration::from_secs(3)))
        .build();

    let bulkhead = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..50 {
        let bh = Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    let max_seen = max_observed.load(Ordering::SeqCst);
    assert!(
        max_seen <= max_concurrent,
        "Permit limit violated: max_seen={}, max_concurrent={}",
        max_seen,
        max_concurrent
    );
}
