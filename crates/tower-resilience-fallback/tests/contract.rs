//! Tower `Service` contract regression for `Fallback`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::service_fn;
use tower::Layer;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::{
    ControlledService, ControlledServiceClosed, ServiceProbe, StatefulInner,
};
use tower_resilience_fallback::{FallbackError, FallbackLayer};

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrimaryError(&'static str);

async fn wait_for<F>(message: &'static str, mut condition: F)
where
    F: FnMut() -> bool,
{
    tokio::time::timeout(Duration::from_secs(1), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{message}"));
}

#[tokio::test]
async fn fallback_drives_readied_instance() {
    let layer = FallbackLayer::<(), (), std::convert::Infallible>::value(());
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn fallback_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = FallbackLayer::<(), (), std::convert::Infallible>::value(());
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn generic_backup_composes_without_erasure_and_drives_readied_instance() {
    let backup = ServiceProbe::new(ConcurrencyLimit::new(StatefulInner::new(), 1));
    let backup_probe = backup.handle();
    let primary = service_fn(|(): ()| async { Err::<(), _>(PrimaryError("primary")) });
    let layer = FallbackLayer::<(), (), PrimaryError>::tower_service(backup);
    let mut service = layer.layer(primary);

    for _ in 0..3 {
        service.ready().await.unwrap().call(()).await.unwrap();
    }

    let snapshot = backup_probe.snapshot();
    assert_eq!(snapshot.calls, 3);
    assert_eq!(snapshot.completed, 3);
    backup_probe.assert_ready_contract();
    backup_probe.assert_quiescent();
}

#[tokio::test]
async fn generic_backup_waits_for_readiness_before_calling() {
    let (controlled, controller) = ControlledService::new(false);
    let backup = ServiceProbe::new(controlled);
    let backup_probe = backup.handle();
    let primary =
        service_fn(
            |request: &'static str| async move { Err::<&'static str, _>(PrimaryError(request)) },
        );
    let service = FallbackLayer::<&'static str, &'static str, PrimaryError>::tower_service(backup)
        .layer(primary);

    let call = tokio::spawn(service.oneshot("request"));
    wait_for("backup readiness was not polled", || {
        backup_probe.snapshot().readiness_pending > 0
    })
    .await;
    assert_eq!(backup_probe.snapshot().calls, 0);

    controller.set_ready(true);
    wait_for("ready backup was not called", || {
        backup_probe.snapshot().calls == 1
    })
    .await;
    controller.allow(1);

    assert_eq!(call.await.unwrap().unwrap(), "request");
    backup_probe.assert_ready_contract();
    backup_probe.assert_quiescent();
}

#[tokio::test]
async fn generic_backup_propagates_readiness_error_as_fallback_error() {
    let (controlled, controller) = ControlledService::new(false);
    let backup = ServiceProbe::new(controlled);
    let backup_probe = backup.handle();
    controller.close();
    let primary =
        service_fn(|_request: String| async move { Err::<String, _>(PrimaryError("primary")) });
    let service =
        FallbackLayer::<String, String, PrimaryError>::tower_service(backup).layer(primary);

    let error = service.oneshot("request".to_string()).await.unwrap_err();
    assert!(matches!(
        error,
        FallbackError::FallbackFailed(ControlledServiceClosed)
    ));
    assert_eq!(backup_probe.snapshot().readiness_errors, 1);
    assert_eq!(backup_probe.snapshot().calls, 0);
    backup_probe.assert_ready_contract();
}

#[tokio::test]
async fn skipped_primary_error_is_preserved_and_backup_is_untouched() {
    let (controlled, _controller) = ControlledService::new(true);
    let backup = ServiceProbe::new(controlled);
    let backup_probe = backup.handle();
    let primary = service_fn(|(): ()| async { Err::<(), _>(PrimaryError("preserved")) });
    let service = FallbackLayer::<(), (), PrimaryError>::tower_service(backup)
        .handle(|_| false)
        .layer(primary);

    let error = service.oneshot(()).await.unwrap_err();
    assert_eq!(error.primary_error(), Some(&PrimaryError("preserved")));
    assert!(error.fallback_error().is_none());
    assert_eq!(backup_probe.snapshot().readiness_polls, 0);
    assert_eq!(backup_probe.snapshot().calls, 0);
}

#[tokio::test]
async fn backup_call_error_is_authoritative_and_may_have_a_different_type() {
    #[derive(Debug, Eq, PartialEq)]
    struct BackupError(&'static str);

    let primary = service_fn(|(): ()| async { Err::<(), _>(PrimaryError("primary")) });
    let backup = service_fn(|(): ()| async { Err::<(), _>(BackupError("backup")) });
    let service = FallbackLayer::<(), (), PrimaryError>::tower_service(backup).layer(primary);

    let error = service.oneshot(()).await.unwrap_err();
    assert!(error.primary_error().is_none());
    assert_eq!(error.fallback_error(), Some(&BackupError("backup")));
}

#[tokio::test]
async fn response_predicate_can_delegate_to_generic_backup() {
    let primary =
        service_fn(|_request: &'static str| async { Ok::<_, PrimaryError>("stale-primary") });
    let backup =
        service_fn(
            |request: &'static str| async move { Ok::<_, std::convert::Infallible>(request) },
        );
    let service = FallbackLayer::<&'static str, &'static str, PrimaryError>::tower_service(backup)
        .handle_response(|response| *response == "stale-primary")
        .layer(primary);

    assert_eq!(
        service.oneshot("backup-response").await.unwrap(),
        "backup-response"
    );
}

#[tokio::test]
async fn cancelling_pending_backup_readiness_releases_shared_backup() {
    let (controlled, controller) = ControlledService::new(false);
    let backup = ServiceProbe::new(controlled);
    let backup_probe = backup.handle();
    let primary =
        service_fn(
            |request: &'static str| async move { Err::<&'static str, _>(PrimaryError(request)) },
        );
    let layer = FallbackLayer::<&'static str, &'static str, PrimaryError>::tower_service(backup);
    let service = layer.layer(primary);

    let cancelled = tokio::spawn(service.clone().oneshot("cancelled"));
    wait_for("cancelled call never reached backup readiness", || {
        backup_probe.snapshot().readiness_pending > 0
    })
    .await;
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());

    controller.set_ready(true);
    let replacement = tokio::spawn(service.oneshot("replacement"));
    wait_for("replacement could not acquire shared backup", || {
        backup_probe.snapshot().calls == 1
    })
    .await;
    controller.allow(1);
    assert_eq!(replacement.await.unwrap().unwrap(), "replacement");

    backup_probe.assert_ready_contract();
    backup_probe.assert_quiescent();
}

#[tokio::test]
async fn cancelling_backup_call_drops_work_and_allows_clone_to_continue() {
    let (controlled, controller) = ControlledService::new(true);
    let backup = ServiceProbe::new(controlled);
    let backup_probe = backup.handle();
    let primary =
        service_fn(
            |request: &'static str| async move { Err::<&'static str, _>(PrimaryError(request)) },
        );
    let layer = FallbackLayer::<&'static str, &'static str, PrimaryError>::tower_service(backup);
    let service = layer.layer(primary);

    let cancelled = tokio::spawn(service.clone().oneshot("cancelled"));
    wait_for("cancelled call did not reach backup", || {
        backup_probe.snapshot().calls == 1
    })
    .await;
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    wait_for("backup call future was not cancelled", || {
        backup_probe.snapshot().cancelled == 1
    })
    .await;

    let replacement = tokio::spawn(service.oneshot("replacement"));
    wait_for("replacement did not reach backup", || {
        backup_probe.snapshot().calls == 2
    })
    .await;
    controller.allow(1);
    assert_eq!(replacement.await.unwrap().unwrap(), "replacement");

    let snapshot = backup_probe.snapshot();
    assert_eq!(snapshot.cancelled, 1);
    assert_eq!(snapshot.completed, 1);
    backup_probe.assert_ready_contract();
    backup_probe.assert_quiescent();
}
