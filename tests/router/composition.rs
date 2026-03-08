//! Tests for composing the router with other resilience patterns.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::util::BoxService;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_router::WeightedRouter;
use tower_resilience_timelimiter::TimeLimiterLayer;

type BoxSvc<E> = BoxService<String, String, E>;

#[derive(Debug)]
struct AppError(String);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AppError {}

#[tokio::test]
async fn router_with_timelimiter_per_backend() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);

    let tl = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let svc = tower::service_fn(move |req: String| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(format!("resp: {req}"))
        }
    });

    let wrapped: BoxSvc<_> = BoxService::new(tl.layer(svc));

    let mut router = WeightedRouter::builder().route(wrapped, 1).build();

    let resp: String = router
        .ready()
        .await
        .unwrap()
        .call("hello".into())
        .await
        .unwrap();
    assert_eq!(resp, "resp: hello");
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn router_with_circuitbreaker_per_backend() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let cb = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .build();

    let svc = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(req)
        }
    });

    let wrapped: BoxSvc<_> = BoxService::new(cb.layer(svc));

    let mut router = WeightedRouter::builder().route(wrapped, 1).build();

    for _ in 0..10 {
        let _ = router.ready().await.unwrap().call("test".into()).await;
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn router_distributes_across_circuit_broken_backends() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let ca = Arc::clone(&count_a);
    let svc_a = tower::service_fn(move |req: String| {
        let c = Arc::clone(&ca);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(format!("a: {req}"))
        }
    });

    let cb_count = Arc::clone(&count_b);
    let svc_b = tower::service_fn(move |req: String| {
        let c = Arc::clone(&cb_count);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(format!("b: {req}"))
        }
    });

    let cb_layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .build();

    let wrapped_a: BoxSvc<_> = BoxService::new(cb_layer.layer(svc_a));
    let wrapped_b: BoxSvc<_> = BoxService::new(cb_layer.layer(svc_b));

    let mut router = WeightedRouter::builder()
        .route(wrapped_a, 80)
        .route(wrapped_b, 20)
        .build();

    for _ in 0..100 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(count_a.load(Ordering::SeqCst), 80);
    assert_eq!(count_b.load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn router_with_bulkhead_per_backend() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);

    let bh = BulkheadLayer::builder().max_concurrent_calls(10).build();

    let svc = tower::service_fn(move |req: String| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(req)
        }
    });

    let wrapped: BoxSvc<_> = BoxService::new(bh.layer(svc));

    let mut router = WeightedRouter::builder().route(wrapped, 1).build();

    for _ in 0..20 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(count.load(Ordering::SeqCst), 20);
}
