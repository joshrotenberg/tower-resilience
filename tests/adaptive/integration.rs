//! Integration tests for basic adaptive limiter functionality.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, Vegas};

#[tokio::test]
async fn test_basic_request_passes_through() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, &str>(format!("response: {}", req))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_multiple_sequential_requests() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, &str>(req * 2)
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    for i in 1..=5 {
        let response = service.ready().await.unwrap().call(i).await.unwrap();
        assert_eq!(response, i * 2);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_error_propagates() {
    let service = tower::service_fn(|_req: ()| async move { Err::<(), _>("expected error") });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_service_clone() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, &str>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    let mut svc1 = service.clone();
    let mut svc2 = service.clone();

    let r1 = svc1
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    let r2 = svc2
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    assert_eq!(r1, "response: a");
    assert_eq!(r2, "response: b");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_with_vegas_algorithm() {
    let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Vegas::builder().initial_limit(10).build(),
        ))
        .service(service);

    let response = service.ready().await.unwrap().call(21).await.unwrap();
    assert_eq!(response, 42);
}

#[tokio::test]
async fn test_layer_builder() {
    let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req) });

    let layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(5)
            .min_limit(1)
            .max_limit(50)
            .increase_by(2)
            .decrease_factor(0.75)
            .latency_threshold(Duration::from_millis(500))
            .build(),
    );

    let mut service = layer.layer(service);

    let response = service.ready().await.unwrap().call(42).await.unwrap();
    assert_eq!(response, 42);
}
