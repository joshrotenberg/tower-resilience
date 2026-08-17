//! Tower `Service` contract regression for `Retry`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{service_fn, Service, ServiceExt};
use tower_resilience_core::testing::{
    ControlledService, ControlledServiceClosed, ServiceProbe, StatefulInner,
};
use tower_resilience_retry::RetryLayer;

#[tokio::test]
async fn retry_drives_readied_instance() {
    let layer: tower_resilience_retry::RetryLayer<(), (), std::convert::Infallible> =
        RetryLayer::builder().max_attempts(1).build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    // Multiple calls also exercise the retry boundary.
    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn retry_repolls_concurrency_limited_inner_before_each_attempt() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_service = Arc::clone(&attempts);
    let inner = service_fn(move |(): ()| {
        let attempt = attempts_for_service.fetch_add(1, Ordering::SeqCst);
        async move {
            if attempt < 2 {
                Err("retryable")
            } else {
                Ok(())
            }
        }
    });
    let probe = ServiceProbe::new(inner);
    let probe_handle = probe.handle();
    let inner = ConcurrencyLimit::new(probe, 1);
    let layer: tower_resilience_retry::RetryLayer<(), (), &'static str> = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::ZERO)
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    svc.ready().await.unwrap().call(()).await.unwrap();

    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let snapshot = probe_handle.snapshot();
    assert_eq!(snapshot.calls, 3);
    assert_eq!(snapshot.readiness_successes, 3);
    probe_handle.assert_ready_contract();
    probe_handle.assert_quiescent();
}

#[tokio::test]
async fn retry_readiness_error_is_terminal_and_preserved() {
    let (inner, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(inner);
    let probe_handle = probe.handle();
    let predicate_calls = Arc::new(AtomicUsize::new(0));
    let predicate_calls_for_retry = Arc::clone(&predicate_calls);
    let layer: tower_resilience_retry::RetryLayer<
        &'static str,
        &'static str,
        ControlledServiceClosed,
    > = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::ZERO)
        .retry_on(move |_| {
            predicate_calls_for_retry.fetch_add(1, Ordering::SeqCst);
            true
        })
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(probe);

    let future = svc.ready().await.unwrap().call("request");
    controller.close();
    let error = future.await.unwrap_err();

    assert_eq!(error, ControlledServiceClosed);
    assert_eq!(controller.calls(), 1, "readiness failure must prevent call");
    assert_eq!(
        predicate_calls.load(Ordering::SeqCst),
        1,
        "the retry predicate applies to the call error, not readiness errors"
    );
    let snapshot = probe_handle.snapshot();
    assert_eq!(snapshot.calls, 1);
    assert_eq!(snapshot.readiness_successes, 1);
    assert_eq!(snapshot.readiness_errors, 1);
    probe_handle.assert_ready_contract();
    probe_handle.assert_quiescent();
}
