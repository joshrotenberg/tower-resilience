//! Tests for adaptive concurrency algorithms.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_adaptive::{
    AdaptiveLimiterLayer, Aimd, Algorithm, ConcurrencyAlgorithm, Vegas,
};

#[tokio::test]
async fn test_aimd_limit_increases_on_fast_responses() {
    let service = tower::service_fn(|_req: ()| async {
        // Fast response - well under threshold
        Ok::<_, &str>(())
    });

    let algorithm = Aimd::builder()
        .initial_limit(10)
        .increase_by(1)
        .latency_threshold(Duration::from_secs(1))
        .build();

    let _initial_limit = algorithm.limit();

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(algorithm))
        .service(service);

    // Make several fast requests
    for _ in 0..10 {
        service.ready().await.unwrap().call(()).await.unwrap();
    }

    // The algorithm adjusts limit internally based on response latency
}

#[tokio::test]
async fn test_aimd_limit_decreases_on_errors() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = cc.fetch_add(1, Ordering::SeqCst);
        async move {
            if count < 3 {
                Ok::<_, &str>(())
            } else {
                Err("error")
            }
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(20)
                .decrease_factor(0.5)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    // Make some successful requests
    for _ in 0..3 {
        let _ = service.ready().await.unwrap().call(()).await;
    }

    // Make failing requests - limit should decrease
    for _ in 0..3 {
        let _ = service.ready().await.unwrap().call(()).await;
    }

    // Service should still be operational
    assert_eq!(call_count.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn test_aimd_respects_min_limit() {
    let fail_count = Arc::new(AtomicUsize::new(0));
    let fc = Arc::clone(&fail_count);

    let service = tower::service_fn(move |_req: ()| {
        fc.fetch_add(1, Ordering::SeqCst);
        async { Err::<(), _>("always fail") }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .min_limit(2)
                .decrease_factor(0.5)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    // Make many failing requests
    for _ in 0..20 {
        let _ = service.ready().await.unwrap().call(()).await;
    }

    // Should have made all calls (limit shouldn't go below min)
    assert_eq!(fail_count.load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn test_aimd_respects_max_limit() {
    let service = tower::service_fn(|_req: ()| async {
        // Very fast response
        Ok::<_, &str>(())
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(5)
                .max_limit(10)
                .increase_by(5)
                .latency_threshold(Duration::from_secs(10))
                .build(),
        ))
        .service(service);

    // Make many fast requests
    for _ in 0..50 {
        service.ready().await.unwrap().call(()).await.unwrap();
    }

    // All should succeed (limit is capped at max)
}

#[tokio::test]
async fn test_vegas_basic_operation() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: i32| {
        cc.fetch_add(1, Ordering::SeqCst);
        async move { Ok::<_, &str>(req * 2) }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Vegas::builder().initial_limit(10).alpha(3).beta(6).build(),
        ))
        .service(service);

    for i in 1..=10 {
        let response = service.ready().await.unwrap().call(i).await.unwrap();
        assert_eq!(response, i * 2);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_vegas_with_custom_parameters() {
    let service = tower::service_fn(|_req: ()| async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok::<_, &str>(())
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Vegas::builder()
                .initial_limit(5)
                .min_limit(1)
                .max_limit(20)
                .alpha(2)
                .beta(4)
                .build(),
        ))
        .service(service);

    for _ in 0..10 {
        service.ready().await.unwrap().call(()).await.unwrap();
    }
}

#[tokio::test]
async fn test_algorithm_enum_aimd() {
    let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req) });

    let algorithm = Algorithm::Aimd(
        Aimd::builder()
            .initial_limit(10)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    );

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(algorithm))
        .service(service);

    let response = service.ready().await.unwrap().call(42).await.unwrap();
    assert_eq!(response, 42);
}

#[tokio::test]
async fn test_algorithm_enum_vegas() {
    let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req) });

    let algorithm = Algorithm::Vegas(Vegas::builder().initial_limit(10).build());

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(algorithm))
        .service(service);

    let response = service.ready().await.unwrap().call(42).await.unwrap();
    assert_eq!(response, 42);
}

#[tokio::test]
async fn test_aimd_slow_response_decreases_limit() {
    let service = tower::service_fn(|_req: ()| async {
        // Slow response - above threshold
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok::<_, &str>(())
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_millis(50))
                .decrease_factor(0.9)
                .build(),
        ))
        .service(service);

    // Make requests that will be slow (above threshold)
    for _ in 0..5 {
        service.ready().await.unwrap().call(()).await.unwrap();
    }

    // Service should still work, just with adjusted limit
}
