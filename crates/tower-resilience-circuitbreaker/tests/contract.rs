//! Tower `Service` contract regression for `CircuitBreaker`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};
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

/// Builds a breaker configured to reach `HalfOpen` quickly and to admit
/// exactly one trial probe per half-open cycle.
fn half_open_probe_layer(
    backpressure: bool,
    wait_duration_in_open: Duration,
) -> tower_resilience_circuitbreaker::CircuitBreakerConfigBuilder<DefaultClassifier> {
    let builder = CircuitBreakerLayer::builder()
        .wait_duration_in_open(wait_duration_in_open)
        .sliding_window_size(1)
        .minimum_number_of_calls(1)
        .permitted_calls_in_half_open(1);
    if backpressure {
        builder.backpressure()
    } else {
        builder
    }
}

/// Regression for #382: half-open admission must reserve a probe slot
/// atomically, not compare against completed-call counts. Under the old
/// implementation every one of these concurrent clones observed the same
/// (zero) completed count and all were admitted.
#[tokio::test]
async fn half_open_admission_never_exceeds_permitted_calls_under_contention() {
    use futures::future::join_all;
    use tokio::sync::Barrier;

    const CLONES: usize = 8;

    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let service = half_open_probe_layer(false, Duration::from_millis(10))
        .build()
        .layer(probe);
    service.force_open().await;
    // Let the open-circuit cooldown elapse so the first admission attempt
    // transitions Open -> HalfOpen and reserves the batch's only slot.
    tokio::time::sleep(Duration::from_millis(20)).await;

    let barrier = Arc::new(Barrier::new(CLONES));
    let tasks = (0..CLONES)
        .map(|_| {
            let mut clone = service.clone();
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                match ServiceExt::<&'static str>::ready(&mut clone).await {
                    Ok(ready) => ready.call("probe").await,
                    Err(error) => Err(error),
                }
            })
        })
        .collect::<Vec<_>>();

    // Wait for the single admitted probe to reach the inner service.
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().calls == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("no probe ever reached the inner service");

    // Wait for every other clone's attempt to resolve (rejection is
    // synchronous once the shared permit is gone -- no inner wait needed).
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let finished = tasks.iter().filter(|task| task.is_finished()).count();
            if finished >= CLONES - 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("rejected clones never resolved");

    // Exactly one probe is reserved and the batch has not yet resolved, so
    // the circuit must still be HalfOpen.
    assert_eq!(handle.snapshot().calls, 1);
    assert_eq!(
        service.state().await,
        tower_resilience_circuitbreaker::CircuitState::HalfOpen
    );

    // Release the single admitted probe and collect every outcome.
    controller.allow(1);
    let results = join_all(tasks).await;
    let admitted = results
        .into_iter()
        .map(|result| result.expect("task panicked"))
        .filter(|result| result.is_ok())
        .count();

    assert_eq!(
        admitted, 1,
        "expected exactly one admitted half-open probe out of {CLONES} concurrent clones"
    );
    assert_eq!(handle.snapshot().calls, 1);
}

/// Regression for #382: dropping an admitted probe's `call` future before it
/// completes must return its reservation for reuse, and must not record a
/// success or failure for the cancelled attempt.
#[tokio::test]
async fn dropped_half_open_probe_future_releases_its_slot_without_recording_a_result() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let service = half_open_probe_layer(false, Duration::from_millis(10))
        .build()
        .layer(probe);
    service.force_open().await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Admit the only half-open slot and never let it complete. `call`
    // returns a lazy future -- it must actually be driven (spawned) for the
    // admission check inside it to run at all.
    let mut first = service.clone();
    let first_task = tokio::spawn(async move {
        ServiceExt::<&'static str>::ready(&mut first)
            .await
            .unwrap()
            .call("first")
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().calls == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first probe never reached the inner service");

    // A second clone is rejected while the only slot is still reserved.
    let mut second = service.clone();
    let rejected = ServiceExt::<&'static str>::ready(&mut second)
        .await
        .unwrap()
        .call("second")
        .await;
    assert!(matches!(rejected, Err(CircuitBreakerError::OpenCircuit)));
    assert_eq!(handle.snapshot().calls, 1);

    // Cancel the first probe before it ever resolves.
    first_task.abort();
    let _ = first_task.await;

    // The slot must now be available for a fresh probe.
    controller.allow(1);
    let mut third = service.clone();
    let admitted = ServiceExt::<&'static str>::ready(&mut third)
        .await
        .unwrap()
        .call("third")
        .await;
    assert_eq!(admitted.unwrap(), "third");

    let snapshot = handle.snapshot();
    assert_eq!(snapshot.calls, 2, "first (cancelled) and third (completed)");
    assert_eq!(snapshot.cancelled, 1);
    assert_eq!(snapshot.completed, 1);
}

/// Regression for #382: a backpressure-mode caller blocked because the
/// half-open batch is full must be woken promptly when a reservation is
/// released by cancellation, not only after the (much longer) open-circuit
/// cooldown timer elapses again.
#[tokio::test]
async fn half_open_backpressure_waiter_wakes_promptly_when_a_slot_frees() {
    const WAIT_DURATION_IN_OPEN: Duration = Duration::from_millis(300);

    let (controlled, _controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let service = half_open_probe_layer(true, WAIT_DURATION_IN_OPEN)
        .build()
        .layer(probe);
    service.force_open().await;
    tokio::time::sleep(WAIT_DURATION_IN_OPEN + Duration::from_millis(10)).await;

    let mut first = service.clone();
    let first_task = tokio::spawn(async move {
        ServiceExt::<&'static str>::ready(&mut first)
            .await
            .unwrap()
            .call("first")
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().calls == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first probe never reached the inner service");

    // A second clone blocks in backpressure mode: the batch is full.
    let mut second = service.clone();
    let waiter = tokio::spawn(async move {
        let started = Instant::now();
        let ready = ServiceExt::<&'static str>::ready(&mut second).await;
        (ready.is_ok(), started.elapsed())
    });

    // Give the waiter a chance to register and go Pending.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Free the reserved slot by cancelling the first probe, well before the
    // 300ms open-circuit cooldown would otherwise elapse again.
    first_task.abort();
    let _ = first_task.await;

    let (became_ready, elapsed) = tokio::time::timeout(Duration::from_millis(200), waiter)
        .await
        .expect("half-open waiter was not woken promptly after a slot freed")
        .expect("waiter task panicked");

    assert!(became_ready, "waiter did not observe readiness");
    assert!(
        elapsed < WAIT_DURATION_IN_OPEN,
        "waiter took {elapsed:?}, no faster than the open-circuit cooldown of {WAIT_DURATION_IN_OPEN:?}"
    );
}
