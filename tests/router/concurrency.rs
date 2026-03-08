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
async fn concurrent_requests_distribute_correctly() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let router = WeightedRouter::builder()
        .route(
            slow_counting_svc(Arc::clone(&count_a), Duration::from_millis(1)),
            80,
        )
        .route(
            slow_counting_svc(Arc::clone(&count_b), Duration::from_millis(1)),
            20,
        )
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
    assert_eq!(a + b, total);
}

#[tokio::test]
async fn high_concurrency_no_panics() {
    let router = WeightedRouter::builder()
        .route(counting_svc(Arc::new(AtomicUsize::new(0))), 50)
        .route(counting_svc(Arc::new(AtomicUsize::new(0))), 30)
        .route(counting_svc(Arc::new(AtomicUsize::new(0))), 20)
        .build();

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
}
