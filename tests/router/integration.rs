//! Basic integration tests for the weighted router.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::util::BoxService;
use tower::{Service, ServiceExt};
use tower_resilience_router::{SelectionStrategy, WeightedRouter};

type BoxSvc = BoxService<String, String, TestError>;

#[derive(Debug)]
struct TestError(String);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TestError {}

fn counting_svc(counter: Arc<AtomicUsize>, label: &'static str) -> BoxSvc {
    BoxService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&counter);
        let l = label;
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("{l}: {req}"))
        }
    }))
}

fn failing_svc(msg: &'static str) -> BoxSvc {
    BoxService::new(tower::service_fn(move |_req: String| async move {
        Err::<String, _>(TestError(msg.to_string()))
    }))
}

#[tokio::test]
async fn deterministic_exact_distribution_two_backends() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count_a), "a"), 70)
        .route(counting_svc(Arc::clone(&count_b), "b"), 30)
        .build();

    for _ in 0..100 {
        let resp = router.ready().await.unwrap().call("x".into()).await;
        assert!(resp.is_ok());
    }

    assert_eq!(count_a.load(Ordering::SeqCst), 70);
    assert_eq!(count_b.load(Ordering::SeqCst), 30);
}

#[tokio::test]
async fn deterministic_exact_distribution_three_backends() {
    let counts: Vec<Arc<AtomicUsize>> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&counts[0]), "a"), 60)
        .route(counting_svc(Arc::clone(&counts[1]), "b"), 30)
        .route(counting_svc(Arc::clone(&counts[2]), "c"), 10)
        .build();

    for _ in 0..200 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    // Two full cycles of 100
    assert_eq!(counts[0].load(Ordering::SeqCst), 120);
    assert_eq!(counts[1].load(Ordering::SeqCst), 60);
    assert_eq!(counts[2].load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn random_converges_to_weights() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count_a), "a"), 75)
        .route(counting_svc(Arc::clone(&count_b), "b"), 25)
        .random()
        .build();

    let total = 10_000;
    for _ in 0..total {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    let a = count_a.load(Ordering::SeqCst);
    let ratio = a as f64 / total as f64;
    assert!(
        (0.70..=0.80).contains(&ratio),
        "expected ~75%, got {:.1}%",
        ratio * 100.0
    );
}

#[tokio::test]
async fn single_backend_receives_all_traffic() {
    let count = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count), "only"), 1)
        .build();

    for _ in 0..100 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(count.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn error_from_backend_propagates() {
    let mut router = WeightedRouter::builder()
        .route(failing_svc("backend down"), 1)
        .build();

    let result = router.ready().await.unwrap().call("x".into()).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().0, "backend down");
}

#[tokio::test]
async fn response_includes_correct_content() {
    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::new(AtomicUsize::new(0)), "v1"), 1)
        .build();

    let resp = router
        .ready()
        .await
        .unwrap()
        .call("hello".into())
        .await
        .unwrap();
    assert_eq!(resp, "v1: hello");
}

#[tokio::test]
async fn equal_weights_distribute_evenly() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count_a), "a"), 1)
        .route(counting_svc(Arc::clone(&count_b), "b"), 1)
        .build();

    for _ in 0..100 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(count_a.load(Ordering::SeqCst), 50);
    assert_eq!(count_b.load(Ordering::SeqCst), 50);
}

#[tokio::test]
async fn event_listener_records_all_routes() {
    let event_count = Arc::new(AtomicUsize::new(0));
    let ec = Arc::clone(&event_count);

    let backend_0_count = Arc::new(AtomicUsize::new(0));
    let b0 = Arc::clone(&backend_0_count);

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::new(AtomicUsize::new(0)), "a"), 80)
        .route(counting_svc(Arc::new(AtomicUsize::new(0)), "b"), 20)
        .on_request_routed(move |idx, _weight| {
            ec.fetch_add(1, Ordering::SeqCst);
            if idx == 0 {
                b0.fetch_add(1, Ordering::SeqCst);
            }
        })
        .build();

    for _ in 0..100 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(event_count.load(Ordering::SeqCst), 100);
    assert_eq!(backend_0_count.load(Ordering::SeqCst), 80);
}

#[tokio::test]
async fn builder_name_is_accessible() {
    let router = WeightedRouter::builder()
        .name("canary-deploy")
        .route(counting_svc(Arc::new(AtomicUsize::new(0)), "a"), 90)
        .route(counting_svc(Arc::new(AtomicUsize::new(0)), "b"), 10)
        .build();

    assert_eq!(router.name(), "canary-deploy");
    assert_eq!(router.backend_count(), 2);
    assert_eq!(router.weights(), vec![90, 10]);
}

#[tokio::test]
async fn strategy_can_be_set_explicitly() {
    let count = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count), "a"), 1)
        .strategy(SelectionStrategy::Deterministic)
        .build();

    let _ = router.ready().await.unwrap().call("x".into()).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn heavy_skew_routes_correctly() {
    let count_primary = Arc::new(AtomicUsize::new(0));
    let count_canary = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .route(counting_svc(Arc::clone(&count_primary), "primary"), 99)
        .route(counting_svc(Arc::clone(&count_canary), "canary"), 1)
        .build();

    for _ in 0..100 {
        let _ = router.ready().await.unwrap().call("x".into()).await;
    }

    assert_eq!(count_primary.load(Ordering::SeqCst), 99);
    assert_eq!(count_canary.load(Ordering::SeqCst), 1);
}
