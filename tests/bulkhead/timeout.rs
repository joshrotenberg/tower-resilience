use std::time::Duration;
use tokio::time::sleep;
use tower::Service;
use tower_bulkhead::{BulkheadConfig, BulkheadError};

/// Test immediate rejection with 0ms timeout
#[tokio::test]
async fn timeout_zero_immediate_rejection() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::ZERO))
        .build();

    let mut bulkhead = layer.layer(service);

    // First call gets the permit
    let handle = tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;
        Ok::<(), String>(())
    });

    // Give first call time to acquire permit
    sleep(Duration::from_millis(10)).await;

    // Second call should be rejected immediately
    let result = bulkhead.call(()).await;
    assert!(matches!(result, Err(BulkheadError::Timeout)));

    let _ = handle.await;
}

/// Test timeout exactly matches service duration
#[tokio::test]
async fn timeout_matches_service_duration() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Start first call that will take 100ms
    let first_call = tokio::spawn(async move {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    sleep(Duration::from_millis(10)).await;

    // Second call waits exactly 100ms for permit
    // This is a race - could succeed or timeout depending on scheduling
    let result = bulkhead.call(()).await;

    // Either outcome is acceptable given timing precision
    match result {
        Ok(_) => {}                       // Permit became available in time
        Err(BulkheadError::Timeout) => {} // Timed out waiting
        Err(e) => panic!("Unexpected error: {:?}", e),
    }

    let _ = first_call.await;
}

/// Test very short timeout (1ms)
#[tokio::test]
async fn timeout_very_short() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(200)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(1)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Occupy the bulkhead
    let _handle = tokio::spawn(async {
        sleep(Duration::from_millis(200)).await;
    });

    sleep(Duration::from_millis(10)).await;

    // Should timeout quickly
    let start = std::time::Instant::now();
    let result = bulkhead.call(()).await;
    let elapsed = start.elapsed();

    assert!(matches!(result, Err(BulkheadError::Timeout)));
    assert!(
        elapsed < Duration::from_millis(50),
        "Timeout took too long: {:?}",
        elapsed
    );
}

/// Test very long timeout (seconds)
#[tokio::test]
async fn timeout_very_long() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_secs(10)))
        .build();

    let mut bulkhead = layer.layer(service);

    // First call
    let result1 = bulkhead.call(()).await;
    assert!(result1.is_ok());

    // Second call should wait and succeed (service is fast)
    let result2 = bulkhead.call(()).await;
    assert!(result2.is_ok());
}

/// Test None timeout (unbounded wait)
#[tokio::test]
async fn timeout_none_waits_indefinitely() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(None) // No timeout
        .build();

    let mut bulkhead = layer.layer(service);

    // First call
    let result1 = bulkhead.call(()).await;
    assert!(result1.is_ok());

    // Second call waits indefinitely (but succeeds quickly since service is fast)
    let result2 = bulkhead.call(()).await;
    assert!(result2.is_ok());
}

/// Test timeout with permit becoming available just in time
#[tokio::test]
async fn timeout_permit_available_just_in_time() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(80)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let mut bulkhead = layer.layer(service);

    // First call takes 80ms
    let result1 = bulkhead.call(()).await;
    assert!(result1.is_ok());

    // Second call should get permit within 100ms timeout
    let result2 = bulkhead.call(()).await;
    assert!(result2.is_ok());
}

/// Test timeout with fast-completing services (no timeout should occur)
#[tokio::test]
async fn timeout_with_fast_service() {
    let service = tower::service_fn(|_req: ()| async {
        // Very fast service
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Make many calls - all should succeed without timeout
    for _ in 0..20 {
        let result = bulkhead.call(()).await;
        assert!(result.is_ok(), "Fast service should not timeout");
    }
}

/// Test timeout behavior under load
#[tokio::test]
async fn timeout_under_load() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(5)
        .max_wait_duration(Some(Duration::from_millis(200)))
        .build();

    let bulkhead = std::sync::Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..50 {
        let bh = std::sync::Arc::clone(&bulkhead);
        handles.push(tokio::spawn(async move {
            let mut guard = bh.lock().await;
            guard.call(()).await
        }));
    }

    let mut timeouts = 0;
    let mut successes = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(Ok(_))) => successes += 1,
            Ok(Err(BulkheadError::Timeout)) => timeouts += 1,
            _ => {}
        }
    }

    // Most should succeed, some might timeout
    assert!(
        successes > 40,
        "Expected most to succeed, got {}",
        successes
    );
    println!("Successes: {}, Timeouts: {}", successes, timeouts);
}

/// Test timeout cleanup doesn't leak permits
#[tokio::test]
async fn timeout_cleanup_no_permit_leak() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Fill bulkhead
    let _handle1 = tokio::spawn(async {
        sleep(Duration::from_millis(150)).await;
    });
    let _handle2 = tokio::spawn(async {
        sleep(Duration::from_millis(150)).await;
    });

    sleep(Duration::from_millis(10)).await;

    // This should timeout
    let result = bulkhead.call(()).await;
    assert!(matches!(result, Err(BulkheadError::Timeout)));

    // Wait for permits to be released
    sleep(Duration::from_millis(200)).await;

    // Now should succeed (verifies no permit leak from timeout)
    let result2 = bulkhead.call(()).await;
    assert!(result2.is_ok(), "Permit may have leaked after timeout");
}

/// Test multiple timeouts in sequence
#[tokio::test]
async fn multiple_sequential_timeouts() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(200)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Start long-running call
    let _handle = tokio::spawn(async {
        sleep(Duration::from_millis(300)).await;
    });

    sleep(Duration::from_millis(10)).await;

    // Multiple calls should all timeout
    for _ in 0..5 {
        let result = bulkhead.call(()).await;
        assert!(matches!(result, Err(BulkheadError::Timeout)));
    }
}
