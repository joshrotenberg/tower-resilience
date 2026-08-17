//! Tests for different delay modes (latency mode vs parallel mode).

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_hedge::HedgeLayer;

#[tokio::test]
async fn test_parallel_mode_fires_all_immediately() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let all_attempts_started = Arc::new(tokio::sync::Barrier::new(3));

    let cc = Arc::clone(&call_count);
    let barrier = Arc::clone(&all_attempts_started);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let barrier = Arc::clone(&barrier);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // The call cannot finish until every no-delay attempt has started.
            // This proves concurrency without depending on scheduler latency.
            barrier.wait().await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .no_delay() // Parallel mode
        .max_hedged_attempts(3)
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    // All 3 should have been called
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_latency_mode_waits_before_hedge() {
    let call_times = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let ct = Arc::clone(&call_times);

    let service = service_fn(move |_req: String| {
        let ct = Arc::clone(&ct);
        async move {
            ct.lock().await.push(std::time::Instant::now());
            // Primary is slow, hedge will be fast
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    let delay = Duration::from_millis(50);
    let layer = HedgeLayer::builder()
        .delay(delay)
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    // Should have 2 calls (primary + 1 hedge)
    let times = call_times.lock().await;
    assert_eq!(times.len(), 2);

    // Second call should be after the configured delay
    let hedge_delay = times[1].duration_since(times[0]);
    assert!(hedge_delay >= delay, "hedge delay: {:?}", hedge_delay);
}

#[tokio::test]
async fn test_dynamic_delay_function() {
    let call_times = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let ct = Arc::clone(&call_times);

    let service = service_fn(move |_req: String| {
        let ct = Arc::clone(&ct);
        async move {
            ct.lock().await.push(std::time::Instant::now());
            // All slow so hedges will fire
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    // Dynamic delay: 50ms for first hedge, 100ms for second
    let layer = HedgeLayer::builder()
        .delay_fn(|attempt| {
            if attempt == 1 {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(100)
            }
        })
        .max_hedged_attempts(3)
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    let times = call_times.lock().await;
    assert_eq!(times.len(), 3);

    // First hedge after ~50ms
    let first_hedge_delay = times[1].duration_since(times[0]);
    assert!(
        first_hedge_delay >= Duration::from_millis(50),
        "first hedge delay: {:?}",
        first_hedge_delay
    );
    // Second hedge after ~100ms from first hedge
    let second_hedge_delay = times[2].duration_since(times[1]);
    assert!(
        second_hedge_delay >= Duration::from_millis(100),
        "second hedge delay: {:?}",
        second_hedge_delay
    );
}

#[tokio::test]
async fn test_fast_primary_prevents_hedge() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Fast response - completes before hedge delay
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100)) // Hedge fires after 100ms
        .max_hedged_attempts(3)
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    // Give time for potential hedges
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Only primary should have been called since it was fast
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_multiple_hedges_with_increasing_delays() {
    let call_times = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let ct = Arc::clone(&call_times);

    let service = service_fn(move |_req: String| {
        let ct = Arc::clone(&ct);
        async move {
            ct.lock().await.push(std::time::Instant::now());
            // Very slow - all hedges will fire
            tokio::time::sleep(Duration::from_millis(1000)).await;
            Ok::<_, TestError>("success".to_string())
        }
    });

    // Fixed delay for all hedges
    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(30))
        .max_hedged_attempts(4)
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    let times = call_times.lock().await;
    assert_eq!(times.len(), 4);

    // Verify staggered timing
    let total_spread = times[3].duration_since(times[0]);
    // Three configured 30ms delays must produce at least 90ms total spread.
    assert!(
        total_spread >= Duration::from_millis(90),
        "total spread: {:?}",
        total_spread
    );
}
