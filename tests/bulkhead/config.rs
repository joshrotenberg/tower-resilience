use std::time::Duration;
use tower_bulkhead::BulkheadConfig;

/// Test max_concurrent_calls = 1 (minimum valid)
#[tokio::test]
async fn max_concurrent_one() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .build();

    let mut bulkhead = layer.layer(service);

    // Should work with exactly 1 permit
    let result = bulkhead.call(()).await;
    assert!(result.is_ok());
}

/// Test max_concurrent_calls with large value
#[tokio::test]
async fn max_concurrent_large_value() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10000)
        .build();

    let mut bulkhead = layer.layer(service);

    // Should work with large permit count
    let result = bulkhead.call(()).await;
    assert!(result.is_ok());
}

/// Test max_wait_duration = Some(Duration::ZERO)
#[tokio::test]
async fn max_wait_duration_zero() {
    let service = tower::service_fn(|_req: ()| async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::ZERO))
        .build();

    let _bulkhead = layer.layer(service);

    // Configuration should be valid even with zero timeout
}

/// Test max_wait_duration = None (unbounded)
#[tokio::test]
async fn max_wait_duration_none() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(5)
        .max_wait_duration(None)
        .build();

    let mut bulkhead = layer.layer(service);

    // Should work with unbounded wait
    let result = bulkhead.call(()).await;
    assert!(result.is_ok());
}

/// Test builder defaults
#[test]
fn builder_defaults() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    // Should build with defaults
    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10)
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test builder with all options
#[test]
fn builder_with_all_options() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(20)
        .max_wait_duration(Some(Duration::from_secs(5)))
        .name("test-bulkhead")
        .on_call_permitted(|_| {})
        .on_call_rejected(|_| {})
        .on_call_finished(|_| {})
        .on_call_failed(|_| {})
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test builder with custom name
#[test]
fn builder_with_custom_name() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10)
        .name("my-custom-bulkhead")
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test builder with event listeners
#[test]
fn builder_with_event_listeners() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10)
        .on_call_permitted(|_count| {
            // Custom logic
        })
        .on_call_rejected(|_max| {
            // Custom logic
        })
        .on_call_finished(|_duration| {
            // Custom logic
        })
        .on_call_failed(|_duration| {
            // Custom logic
        })
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test configuration is immutable after build
#[tokio::test]
async fn config_immutable_after_build() {
    let service = tower::service_fn(|_req: ()| async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok::<(), String>(())
    });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_secs(1)))
        .build();

    let mut bulkhead = layer.layer(service);

    // Make multiple calls - configuration should remain consistent
    let result1 = bulkhead.call(()).await;
    let result2 = bulkhead.call(()).await;

    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

/// Test very large max_concurrent_calls
#[test]
fn very_large_max_concurrent() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    // Should handle large values (but not usize::MAX as that's impractical)
    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(100_000)
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test very long timeout duration
#[test]
fn very_long_timeout() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10)
        .max_wait_duration(Some(Duration::from_secs(3600))) // 1 hour
        .build();

    let _bulkhead = layer.layer(service);
}

/// Test builder can be reused for multiple bulkheads
#[test]
fn builder_reusable() {
    let service1 = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });
    let service2 = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = BulkheadConfig::<(), String>::builder()
        .max_concurrent_calls(10)
        .build();

    let _bulkhead1 = layer.layer(service1);
    let _bulkhead2 = layer.layer(service2);
}

/// Test different wait durations
#[test]
fn various_wait_durations() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    // Test various durations
    let durations = vec![
        Some(Duration::from_millis(1)),
        Some(Duration::from_millis(100)),
        Some(Duration::from_secs(1)),
        Some(Duration::from_secs(60)),
        None,
    ];

    for duration in durations {
        let layer = BulkheadConfig::<(), String>::builder()
            .max_concurrent_calls(10)
            .max_wait_duration(duration)
            .build();

        let service_clone = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });
        let _bulkhead = layer.layer(service_clone);
    }
}
