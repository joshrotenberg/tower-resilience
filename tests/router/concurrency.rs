//! Concurrency and stress tests for the weighted router.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::util::BoxCloneService;
use tower::{Service, ServiceExt};
use tower_resilience_router::WeightedRouter;

type BoxSvc = BoxCloneService<String, String, TestError>;

#[derive(Debug, Clone)]
struct TestError;

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "test error")
    }
}

impl std::error::Error for TestError {}

fn counting_svc(counter: Arc<AtomicUsize>) -> BoxSvc {
    BoxCloneService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&counter);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(req)
        }
    }))
}

fn slow_counting_svc(counter: Arc<AtomicUsize>, delay: Duration) -> BoxSvc {
    BoxCloneService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&counter);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            Ok::<_, TestError>(req)
        }
    }))
}

#[tokio::test]
async fn clone_per_request_distributes_exactly() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));
    let routed = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let routed_events = Arc::clone(&routed);

    let router = WeightedRouter::builder()
        .route(
            slow_counting_svc(Arc::clone(&count_a), Duration::from_millis(1)),
            80,
        )
        .route(
            slow_counting_svc(Arc::clone(&count_b), Duration::from_millis(1)),
            20,
        )
        .on_request_routed(move |index, _weight| {
            routed_events[index].fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let total = 100;
    let mut handles = vec![];
    for i in 0..total {
        let mut r = router.clone();
        handles.push(tokio::spawn(async move {
            r.ready()
                .await
                .unwrap()
                .call(format!("req-{i}"))
                .await
                .unwrap()
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let a = count_a.load(Ordering::SeqCst);
    let b = count_b.load(Ordering::SeqCst);
    assert_eq!((a, b), (80, 20));
    assert_eq!(
        [
            routed[0].load(Ordering::SeqCst),
            routed[1].load(Ordering::SeqCst)
        ],
        [80, 20]
    );
}

#[tokio::test]
async fn dynamically_constructed_router_distributes_exactly_at_high_concurrency() {
    let weights = [50, 30, 20];
    let counts: Vec<_> = weights
        .iter()
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect();

    let mut builder = WeightedRouter::builder();
    for (&weight, counter) in weights.iter().zip(&counts) {
        builder = builder.route(counting_svc(Arc::clone(counter)), weight);
    }

    let router = builder.build();

    let total = 1000;
    let mut handles = vec![];
    for i in 0..total {
        let mut r = router.clone();
        handles.push(tokio::spawn(async move {
            r.ready()
                .await
                .unwrap()
                .call(format!("req-{i}"))
                .await
                .unwrap()
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        handle.await.unwrap();
        success_count += 1;
    }

    assert_eq!(success_count, total);
    assert_eq!(
        counts
            .iter()
            .map(|count| count.load(Ordering::SeqCst))
            .collect::<Vec<_>>(),
        vec![500, 300, 200]
    );
}

#[tokio::test]
async fn concurrent_errors_do_not_corrupt_state() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&success_count);

    let call_count = Arc::new(AtomicUsize::new(0));
    let fail_count = Arc::clone(&call_count);

    let ok_svc: BoxSvc = BoxCloneService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&sc);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(req)
        }
    }));

    let err_svc: BoxSvc = BoxCloneService::new(tower::service_fn(move |_req: String| {
        let c = Arc::clone(&fail_count);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    }));

    let router = WeightedRouter::builder()
        .route(ok_svc, 50)
        .route(err_svc, 50)
        .build();

    let mut handles = vec![];
    for i in 0..100 {
        let mut r = router.clone();
        handles.push(tokio::spawn(async move {
            r.ready().await.unwrap().call(format!("req-{i}")).await
        }));
    }

    let mut ok = 0;
    let mut err = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }

    assert_eq!(ok + err, 100);
    assert_eq!(success_count.load(Ordering::SeqCst), 50);
    assert_eq!(call_count.load(Ordering::SeqCst), 50);
}
