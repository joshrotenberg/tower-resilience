//! Tower `Service` contract regression for `CircuitBreaker`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::future::poll_fn;
use std::task::Poll;
use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_circuitbreaker::{CircuitBreakerError, DefaultClassifier};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_core::testing::{ControlledService, ControlledServiceClosed, ServiceProbe};

#[tokio::test]
async fn circuitbreaker_drives_readied_instance() {
    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(50.0)
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn circuitbreaker_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(50.0)
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

fn pending_probe_layer(
    backpressure: bool,
) -> tower_resilience_circuitbreaker::CircuitBreakerConfigBuilder<DefaultClassifier> {
    let builder = CircuitBreakerLayer::builder()
        .wait_duration_in_open(Duration::from_millis(20))
        .sliding_window_size(1)
        .minimum_number_of_calls(1);
    if backpressure {
        builder.backpressure()
    } else {
        builder
    }
}

#[tokio::test]
async fn open_rejection_does_not_poll_pending_inner() {
    let (controlled, _controller) = ControlledService::new(false);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut service = pending_probe_layer(false).build().layer(probe);
    service.force_open().await;

    ServiceExt::<()>::ready(&mut service).await.unwrap();
    // Repeated readiness polls preserve the same rejection grant.
    ServiceExt::<()>::ready(&mut service).await.unwrap();
    assert_eq!(handle.snapshot().readiness_polls, 0);

    let result = service.call(()).await;
    assert!(matches!(result, Err(CircuitBreakerError::OpenCircuit)));
    assert_eq!(handle.snapshot().readiness_polls, 0);
    assert_eq!(handle.snapshot().calls, 0);
}

#[tokio::test]
async fn open_fallback_does_not_poll_pending_inner() {
    let (controlled, _controller) = ControlledService::new(false);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut service = pending_probe_layer(false)
        .build()
        .layer(probe)
        .with_fallback(|request: &'static str| Box::pin(async move { Ok(request) }));
    service.force_open().await;

    let response = service
        .ready()
        .await
        .unwrap()
        .call("fallback")
        .await
        .unwrap();

    assert_eq!(response, "fallback");
    assert_eq!(handle.snapshot().readiness_polls, 0);
    assert_eq!(handle.snapshot().calls, 0);
}

#[tokio::test]
async fn cloned_open_breakers_reject_without_polling_inner() {
    let (controlled, _controller) = ControlledService::new(false);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut first = pending_probe_layer(false).build().layer(probe);
    first.force_open().await;
    let mut second = first.clone();

    let first_result = ServiceExt::<()>::ready(&mut first)
        .await
        .unwrap()
        .call(())
        .await;
    let second_result = ServiceExt::<()>::ready(&mut second)
        .await
        .unwrap()
        .call(())
        .await;

    assert!(matches!(
        first_result,
        Err(CircuitBreakerError::OpenCircuit)
    ));
    assert!(matches!(
        second_result,
        Err(CircuitBreakerError::OpenCircuit)
    ));
    assert_eq!(handle.snapshot().readiness_polls, 0);
    assert_eq!(handle.snapshot().calls, 0);
}

#[tokio::test]
async fn backpressure_waits_on_circuit_before_pending_inner() {
    let (controlled, _controller) = ControlledService::new(false);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut service = pending_probe_layer(true).build().layer(probe);
    service.force_open().await;

    let initially_pending = poll_fn(|cx| {
        Poll::Ready(matches!(
            Service::<()>::poll_ready(&mut service, cx),
            Poll::Pending
        ))
    })
    .await;
    assert!(initially_pending);
    assert_eq!(handle.snapshot().readiness_polls, 0);

    let task = tokio::spawn(async move {
        let _ = ServiceExt::<()>::ready(&mut service).await;
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().readiness_pending == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("circuit timer did not wake readiness to re-check the inner service");

    assert_eq!(handle.snapshot().readiness_pending, 1);
    task.abort();
    task.await.unwrap_err();
}

#[tokio::test]
async fn admitted_inner_readiness_error_is_preserved() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut service = pending_probe_layer(false).build().layer(probe);
    controller.close();

    let error = match ServiceExt::<()>::ready(&mut service).await {
        Ok(_) => panic!("closed inner unexpectedly became ready"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        CircuitBreakerError::Inner(ControlledServiceClosed)
    ));
    assert_eq!(handle.snapshot().readiness_errors, 1);
}
