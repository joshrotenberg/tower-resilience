//! Tower `Service` contract regression for `AdaptiveService`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_adaptive::{
    AdaptiveError, AdaptiveLimiterLayer, AdaptiveService, Aimd, ConcurrencyAlgorithm,
};
use tower_resilience_core::testing::{ControlledService, ServiceProbe, StatefulInner};

struct ManualLimit {
    limit: AtomicUsize,
    dropped: AtomicUsize,
}

impl ManualLimit {
    fn new(limit: usize) -> Self {
        Self {
            limit: AtomicUsize::new(limit),
            dropped: AtomicUsize::new(0),
        }
    }

    fn set(&self, limit: usize) {
        self.limit.store(limit, Ordering::SeqCst);
    }

    fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }
}

impl ConcurrencyAlgorithm for ManualLimit {
    fn record_success(&self, _latency: Duration) {}

    fn record_failure(&self) {}

    fn record_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }

    fn limit(&self) -> usize {
        self.limit.load(Ordering::SeqCst)
    }

    fn min_limit(&self) -> usize {
        1
    }

    fn max_limit(&self) -> usize {
        usize::MAX
    }
}

async fn wait_for_calls(handle: &tower_resilience_core::testing::ProbeHandle, calls: usize) {
    for _ in 0..1_000 {
        if handle.snapshot().calls == calls {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!(
        "expected {calls} calls, observed {}",
        handle.snapshot().calls
    );
}

#[tokio::test]
async fn adaptive_drives_readied_instance() {
    let layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(8)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    );
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn adaptive_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(8)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    );
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn adaptive_reserves_capacity_across_clones() {
    let (inner, controller) = ControlledService::new(true);
    let inner = ServiceProbe::new(inner);
    let probe = inner.handle();
    let algorithm = Aimd::builder()
        .initial_limit(2)
        .min_limit(2)
        .max_limit(2)
        .build();
    let service = AdaptiveService::new(inner, Arc::new(algorithm));

    let mut tasks = Vec::new();
    for request in 0..8 {
        let mut service = service.clone();
        tasks.push(tokio::spawn(async move {
            ServiceExt::<usize>::ready(&mut service)
                .await
                .unwrap()
                .call(request)
                .await
                .unwrap()
        }));
    }

    wait_for_calls(&probe, 2).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(probe.snapshot().calls, 2);
    assert_eq!(probe.snapshot().peak_in_flight, 2);

    controller.allow(8);
    for task in tasks {
        task.await.unwrap();
    }

    let snapshot = probe.snapshot();
    assert_eq!(snapshot.calls, 8);
    assert_eq!(snapshot.peak_in_flight, 2);
    probe.assert_ready_contract();
    probe.assert_quiescent();
}

#[tokio::test]
async fn dropped_call_future_releases_capacity() {
    let (inner, controller) = ControlledService::new(true);
    let inner = ServiceProbe::new(inner);
    let probe = inner.handle();
    let algorithm = Arc::new(ManualLimit::new(1));
    let service = AdaptiveService::new(inner, Arc::clone(&algorithm));

    let mut first = service.clone();
    let first_future = ServiceExt::<usize>::ready(&mut first)
        .await
        .unwrap()
        .call(1);
    assert_eq!(service.in_flight(), 1);
    drop(first_future);
    assert_eq!(service.in_flight(), 0);
    assert_eq!(algorithm.dropped(), 1);

    let mut second = service.clone();
    let second_future = tokio::time::timeout(
        Duration::from_secs(1),
        ServiceExt::<usize>::ready(&mut second),
    )
    .await
    .expect("dropping an admitted future must wake the next waiter")
    .unwrap()
    .call(2);
    controller.allow(1);
    assert_eq!(second_future.await.unwrap(), 2);

    let snapshot = probe.snapshot();
    assert_eq!(snapshot.cancelled, 1);
    assert_eq!(snapshot.peak_in_flight, 1);
    probe.assert_ready_contract();
    probe.assert_quiescent();
}

#[tokio::test]
async fn dropping_a_readied_clone_releases_its_reservation() {
    let (inner, controller) = ControlledService::new(true);
    let inner = ServiceProbe::new(inner);
    let probe = inner.handle();
    let algorithm = Arc::new(ManualLimit::new(1));
    let service = AdaptiveService::new(inner, algorithm);

    let mut reserved = service.clone();
    ServiceExt::<usize>::ready(&mut reserved).await.unwrap();
    drop(reserved);

    let mut next = service.clone();
    let future = tokio::time::timeout(
        Duration::from_secs(1),
        ServiceExt::<usize>::ready(&mut next),
    )
    .await
    .expect("dropping a readiness grant must wake the next waiter")
    .unwrap()
    .call(7);
    controller.allow(1);
    assert_eq!(future.await.unwrap(), 7);

    assert_eq!(probe.snapshot().peak_in_flight, 1);
    probe.assert_ready_contract();
    probe.assert_quiescent();
}

#[tokio::test]
async fn inner_readiness_error_is_preserved_and_releases_capacity() {
    let (inner, controller) = ControlledService::new(false);
    let inner = ServiceProbe::new(inner);
    let probe = inner.handle();
    let algorithm = Arc::new(ManualLimit::new(1));
    let mut service = AdaptiveService::new(inner, algorithm);
    controller.close();

    match ServiceExt::<usize>::ready(&mut service).await {
        Err(AdaptiveError::Service(_)) => {}
        Err(AdaptiveError::LimitReached) => panic!("inner error was replaced by a limit error"),
        Ok(_) => panic!("closed inner service unexpectedly became ready"),
    }

    let snapshot = probe.snapshot();
    assert_eq!(snapshot.readiness_errors, 1);
    assert_eq!(snapshot.calls, 0);
    assert_eq!(service.in_flight(), 0);
    probe.assert_quiescent();
}

#[tokio::test]
async fn shrinking_limit_retires_in_flight_permits_before_readmitting() {
    let (inner, controller) = ControlledService::new(true);
    let inner = ServiceProbe::new(inner);
    let probe = inner.handle();
    let algorithm = Arc::new(ManualLimit::new(2));
    let service = AdaptiveService::new(inner, Arc::clone(&algorithm));

    let mut first = service.clone();
    let first_future = ServiceExt::<usize>::ready(&mut first)
        .await
        .unwrap()
        .call(1);
    let mut second = service.clone();
    let second_future = ServiceExt::<usize>::ready(&mut second)
        .await
        .unwrap()
        .call(2);
    assert_eq!(probe.snapshot().calls, 2);

    algorithm.set(1);
    let mut third = service.clone();
    let third_task = tokio::spawn(async move {
        ServiceExt::<usize>::ready(&mut third)
            .await
            .unwrap()
            .call(3)
            .await
            .unwrap()
    });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(probe.snapshot().calls, 2);

    controller.allow(1);
    assert_eq!(first_future.await.unwrap(), 1);
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        probe.snapshot().calls,
        2,
        "the first released permit must pay down the shrink"
    );

    controller.allow(1);
    assert_eq!(second_future.await.unwrap(), 2);
    wait_for_calls(&probe, 3).await;
    controller.allow(1);
    assert_eq!(third_task.await.unwrap(), 3);

    assert_eq!(probe.snapshot().peak_in_flight, 2);
    probe.assert_ready_contract();
    probe.assert_quiescent();
}
