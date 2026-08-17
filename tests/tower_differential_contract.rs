//! Differential contract tests against comparable Tower middleware.
//!
//! These tests intentionally compare observable invariants rather than error
//! types or implementation details. Add a scenario here when a
//! tower-resilience feature is a superset of middleware shipped by Tower.

use std::future::poll_fn;
use std::task::Poll;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::{
    ControlledHandle, ControlledService, ProbeHandle, ServiceProbe,
};

#[derive(Debug, Eq, PartialEq)]
struct SinglePermitObservation {
    contender_was_pending: bool,
    calls: usize,
    peak_in_flight: usize,
    readiness_violations: usize,
}

async fn observe_single_permit<S>(
    mut service: S,
    controller: ControlledHandle,
    probe: ProbeHandle,
) -> SinglePermitObservation
where
    S: Service<usize, Response = usize> + Clone,
    S::Error: std::fmt::Debug,
{
    let mut contender = service.clone();
    let first = service.ready().await.unwrap().call(1);

    let contender_was_pending =
        poll_fn(|cx| Poll::Ready(matches!(contender.poll_ready(cx), Poll::Pending))).await;

    controller.allow(1);
    assert_eq!(first.await.unwrap(), 1);

    let second = contender.ready().await.unwrap().call(2);
    controller.allow(1);
    assert_eq!(second.await.unwrap(), 2);

    let snapshot = probe.snapshot();
    probe.assert_ready_contract();
    probe.assert_quiescent();
    SinglePermitObservation {
        contender_was_pending,
        calls: snapshot.calls,
        peak_in_flight: snapshot.peak_in_flight,
        readiness_violations: snapshot.readiness_violations,
    }
}

#[tokio::test]
async fn bulkhead_backpressure_matches_tower_single_permit_admission() {
    use tower_resilience_bulkhead::BulkheadLayer;

    let (tower_inner, tower_controller) = ControlledService::new(true);
    let tower_probe = ServiceProbe::new(tower_inner);
    let tower_handle = tower_probe.handle();
    let tower = tower::limit::ConcurrencyLimit::new(tower_probe, 1);

    let (bulkhead_inner, bulkhead_controller) = ControlledService::new(true);
    let bulkhead_probe = ServiceProbe::new(bulkhead_inner);
    let bulkhead_handle = bulkhead_probe.handle();
    let bulkhead = tower::ServiceBuilder::new()
        .layer(
            BulkheadLayer::builder()
                .max_concurrent_calls(1)
                .backpressure()
                .build(),
        )
        .service(bulkhead_probe);

    let tower_observation = observe_single_permit(tower, tower_controller, tower_handle).await;
    let bulkhead_observation =
        observe_single_permit(bulkhead, bulkhead_controller, bulkhead_handle).await;

    let expected = SinglePermitObservation {
        contender_was_pending: true,
        calls: 2,
        peak_in_flight: 1,
        readiness_violations: 0,
    };
    assert_eq!(tower_observation, expected);
    assert_eq!(bulkhead_observation, expected);
}
