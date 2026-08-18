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
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitState, DefaultClassifier};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_core::testing::{ControlledService, ControlledServiceClosed, ServiceProbe};

#[tokio::test]
async fn circuitbreaker_drives_readied_instance() {
    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .build()
        .unwrap();
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
        .failure_rate_threshold(0.5)
        .build()
        .unwrap();
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
    let mut service = pending_probe_layer(false).build().unwrap().layer(probe);
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
        .unwrap()
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
    let mut first = pending_probe_layer(false).build().unwrap().layer(probe);
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
    let mut service = pending_probe_layer(true).build().unwrap().layer(probe);
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
    let mut service = pending_probe_layer(false).build().unwrap().layer(probe);
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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
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

// ─── Contract coverage for #375: trip-condition and pending-inner paths ───
//
// The tests above cover open-circuit gating (#381) and half-open admission
// (#382). These add the trip-condition contract gaps #375 calls out
// specifically: failure-rate trip, the consecutive-failure model (a
// distinct trip path from the sliding window), slow-call detection, and a
// permanently `Pending` inner service under a `Closed` circuit.

/// Contract coverage for #375: the failure-rate sliding window admits calls
/// while below threshold, trips once the window fills and the rate crosses
/// `failure_rate_threshold`, and a subsequent `poll_ready` reflects `Open`
/// without polling inner again.
#[tokio::test]
async fn failure_rate_threshold_trips_after_window_fills_and_then_rejects_without_polling_inner() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let mut service = CircuitBreakerLayer::builder()
        .failure_classifier(|result: &Result<&'static str, ControlledServiceClosed>| {
            matches!(result, Ok(response) if *response == "fail")
        })
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(4)
        .wait_duration_in_open(Duration::from_secs(60))
        .build()
        .unwrap()
        .layer(probe);

    controller.allow(4);

    // Below the window size, the circuit must stay Closed even though the
    // failures seen so far already exceed the rate threshold.
    for request in ["fail", "fail"] {
        let response = ServiceExt::<&'static str>::ready(&mut service)
            .await
            .unwrap()
            .call(request)
            .await;
        assert_eq!(response.unwrap(), request);
    }
    assert_eq!(service.state().await, CircuitState::Closed);

    // The window fills on the 4th call: 2 successes + 2 failures = 50%,
    // meeting (>=) the configured 50% threshold.
    for request in ["ok", "ok"] {
        let response = ServiceExt::<&'static str>::ready(&mut service)
            .await
            .unwrap()
            .call(request)
            .await;
        assert_eq!(response.unwrap(), request);
    }
    assert_eq!(service.state().await, CircuitState::Open);

    let polls_before = handle.snapshot().readiness_polls;
    let calls_before = handle.snapshot().calls;

    // A subsequent poll_ready/call observes Open directly; inner is never
    // polled or called again.
    ServiceExt::<&'static str>::ready(&mut service)
        .await
        .unwrap();
    let result = service.call("would-be-fine").await;

    assert!(matches!(result, Err(CircuitBreakerError::OpenCircuit)));
    assert_eq!(handle.snapshot().readiness_polls, polls_before);
    assert_eq!(handle.snapshot().calls, calls_before);
}

/// Contract coverage for #375: `FailureModel::ConsecutiveFailures` (the
/// `.consecutive_failures(k)` shortcut) trips on `k` failures in a row,
/// independent of the sliding-window rate path -- it ignores
/// `failure_rate_threshold` and the window/minimum-calls gating entirely.
#[tokio::test]
async fn consecutive_failures_model_trips_independent_of_sliding_window_gating() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let mut service = CircuitBreakerLayer::builder()
        .failure_classifier(|result: &Result<&'static str, ControlledServiceClosed>| {
            matches!(result, Ok(response) if *response == "fail")
        })
        .consecutive_failures(3)
        .wait_duration_in_open(Duration::from_secs(60))
        .build()
        .unwrap()
        .layer(probe);

    controller.allow(10);

    // A leading success, then two failures: only 2 consecutive failures so
    // far -- below k=3. This is also only 3 total calls, nowhere near the
    // default sliding-window minimum_number_of_calls (100), proving this
    // model does not depend on that gating at all.
    for request in ["ok", "fail", "fail"] {
        let response = ServiceExt::<&'static str>::ready(&mut service)
            .await
            .unwrap()
            .call(request)
            .await;
        assert_eq!(response.unwrap(), request);
    }
    assert_eq!(service.state().await, CircuitState::Closed);

    // The 3rd consecutive failure trips immediately, at 4 total calls --
    // far below the sliding-window model's default 100-call minimum.
    let response = ServiceExt::<&'static str>::ready(&mut service)
        .await
        .unwrap()
        .call("fail")
        .await;
    assert_eq!(response.unwrap(), "fail");
    assert_eq!(service.state().await, CircuitState::Open);
}

/// Contract coverage for #375: `slow_call_duration_threshold` /
/// `slow_call_rate_threshold` trip the circuit on latency alone -- every
/// call in this test succeeds (0% failure rate), so only the slow-call path
/// can be responsible for the trip.
#[tokio::test]
async fn slow_call_threshold_trips_independent_of_failure_rate() {
    const SLOW_THRESHOLD: Duration = Duration::from_millis(20);

    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let service = CircuitBreakerLayer::builder()
        .slow_call_duration_threshold(SLOW_THRESHOLD)
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(2)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(60))
        .build()
        .unwrap()
        .layer(probe);

    // First call completes immediately: fast, and (trivially) a success.
    controller.allow(1);
    let mut fast = service.clone();
    let response = ServiceExt::<&'static str>::ready(&mut fast)
        .await
        .unwrap()
        .call("fast")
        .await;
    assert_eq!(response.unwrap(), "fast");
    assert_eq!(service.state().await, CircuitState::Closed);

    // Second call: also a success, but held past SLOW_THRESHOLD before its
    // permit is released, so its recorded duration crosses the slow-call
    // threshold. The future must actually be polled (spawned) for the
    // in-breaker start Instant to begin ticking.
    let mut slow = service.clone();
    let call = tokio::spawn(async move {
        ServiceExt::<&'static str>::ready(&mut slow)
            .await
            .unwrap()
            .call("slow")
            .await
    });
    tokio::time::sleep(SLOW_THRESHOLD * 2).await;
    controller.allow(1);
    let response = call.await.unwrap();
    assert_eq!(response.unwrap(), "slow");

    // 2 calls, 0 failures, 1 slow call: failure rate stays 0% (well under
    // the default 50% failure_rate_threshold) while the slow-call rate hits
    // 50%, meeting slow_call_rate_threshold and tripping the circuit.
    assert_eq!(service.state().await, CircuitState::Open);
}

/// Contract coverage for #375: while the circuit is `Closed`, an inner
/// service that never becomes ready must have its `Pending` forwarded
/// as-is -- no busy-polling (evidenced by the pending-poll count plateauing
/// under repeated scheduler yields, not a sleep), a waker is registered and
/// honored on wake, and dropping a caller's readiness future while inner is
/// still pending is safe and leaves the breaker usable for the next caller.
#[tokio::test]
async fn closed_circuit_forwards_permanently_pending_inner_without_busy_polling() {
    let (controlled, controller) = ControlledService::new(false);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let service = CircuitBreakerLayer::builder().build().unwrap().layer(probe);

    // First caller: registers a readiness waker but is never woken.
    let mut first = service.clone();
    let first_task = tokio::spawn(async move {
        let _ = ServiceExt::<&'static str>::ready(&mut first).await;
    });

    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().readiness_pending == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("readiness was never polled");

    let pending_after_registration = handle.snapshot().readiness_pending;

    // Give the executor many chances to re-poll on its own. Nothing ever
    // wakes this task, so a conforming Closed-state gate must not
    // busy-poll: the pending count must not advance past the single poll
    // that registered the waker. No wall-clock sleep is used here -- the
    // plateau under repeated yields is the evidence.
    for _ in 0..1000 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        handle.snapshot().readiness_pending,
        pending_after_registration,
        "circuit breaker busy-polled a permanently pending inner service"
    );
    assert_eq!(handle.snapshot().calls, 0);

    // Drop the first caller's readiness future while inner is still
    // pending. This must be safe and must not corrupt state for the next
    // caller.
    first_task.abort();
    let _ = first_task.await;

    // Second caller, on a fresh clone: registers its own waker and is woken
    // once inner becomes ready, proving the breaker forwards both the
    // Pending readiness and the eventual wake-up correctly.
    let mut second = service.clone();
    let second_task = tokio::spawn(async move {
        ServiceExt::<&'static str>::ready(&mut second)
            .await
            .unwrap();
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().readiness_pending <= pending_after_registration {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second readiness waiter never registered");

    controller.set_ready(true);
    tokio::time::timeout(Duration::from_secs(1), second_task)
        .await
        .expect("registered readiness waiter was not woken")
        .unwrap();

    handle.assert_quiescent();
}
