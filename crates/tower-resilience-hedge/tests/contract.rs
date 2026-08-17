//! Tower `Service` contract regression for `Hedge`.
//!
//! Hedge fans a single request out to N parallel attempts. The primary uses
//! the caller-readied receiver (correct). Each hedge attempt operates on a
//! fresh `inner.clone()` whose readiness is not inherited from the original
//! (per `tower::limit::ConcurrencyLimit` etc.) and therefore must drive its
//! own `poll_ready` before calling. See #293.
//!
//! The readiness probe below has a `Clone` that resets readiness and a `call`
//! that asserts the inner saw a `poll_ready` since its last `Clone`/call. The
//! primary sleeps long enough for hedges to fire; without the fix the hedge
//! attempt calls into a fresh clone whose `ready` is `false`, and the assert
//! panics.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for related
//! contract tests on simpler layer middleware.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_core::testing::{ControlledService, ProbeHandle, ServiceProbe};
use tower_resilience_core::FnListener;
use tower_resilience_hedge::{HedgeError, HedgeEvent, HedgeLayer};

/// An inner [`Service`] whose [`Clone`] resets readiness, and whose first
/// call sleeps long enough for hedge fan-out to fire (so the hedge attempts
/// actually issue calls against fresh clones).
struct StatefulSlowFirst {
    ready: bool,
    calls: Arc<AtomicUsize>,
}

impl StatefulSlowFirst {
    fn new() -> Self {
        Self {
            ready: false,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Clone for StatefulSlowFirst {
    fn clone(&self) -> Self {
        Self {
            ready: false,
            calls: Arc::clone(&self.calls),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProbeError;

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProbeError")
    }
}

impl std::error::Error for ProbeError {}

async fn wait_for_probe_calls(handle: &ProbeHandle, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.snapshot().calls < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("hedge attempts did not reach the controlled service");
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AttemptError {
    Primary,
    Hedge,
}

impl std::fmt::Display for AttemptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AttemptError {}

#[derive(Debug)]
struct ErrorState {
    calls: AtomicUsize,
    primary: Arc<tokio::sync::Semaphore>,
    hedge: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug)]
struct OrderedErrorService {
    primary: bool,
    state: Arc<ErrorState>,
}

impl OrderedErrorService {
    fn new() -> (Self, Arc<ErrorState>) {
        let state = Arc::new(ErrorState {
            calls: AtomicUsize::new(0),
            primary: Arc::new(tokio::sync::Semaphore::new(0)),
            hedge: Arc::new(tokio::sync::Semaphore::new(0)),
        });
        (
            Self {
                primary: true,
                state: Arc::clone(&state),
            },
            state,
        )
    }
}

impl Clone for OrderedErrorService {
    fn clone(&self) -> Self {
        Self {
            // The exact readied receiver is the primary; all clones are hedge
            // templates or the replacement retained by the outer service.
            primary: false,
            state: Arc::clone(&self.state),
        }
    }
}

impl Service<()> for OrderedErrorService {
    type Response = ();
    type Error = AttemptError;
    type Future = Pin<Box<dyn Future<Output = Result<(), AttemptError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (): ()) -> Self::Future {
        self.state.calls.fetch_add(1, Ordering::SeqCst);
        let primary = self.primary;
        let permit = if primary {
            Arc::clone(&self.state.primary)
        } else {
            Arc::clone(&self.state.hedge)
        };
        Box::pin(async move {
            permit
                .acquire_owned()
                .await
                .expect("test semaphore remains open")
                .forget();
            Err(if primary {
                AttemptError::Primary
            } else {
                AttemptError::Hedge
            })
        })
    }
}

impl Service<()> for StatefulSlowFirst {
    type Response = ();
    type Error = ProbeError;
    type Future = Pin<Box<dyn Future<Output = Result<(), ProbeError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#293)"
        );
        self.ready = false;
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n == 0 {
                // Primary sleeps so the hedge has time to fire.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(())
        })
    }
}

// Bounded with `tokio::time::timeout` so a readiness regression cannot leave
// the test waiting indefinitely for an attempt that never becomes callable.

#[tokio::test]
async fn hedge_drives_readied_instance_on_attempts() {
    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(20))
        .max_hedged_attempts(2)
        .build();
    let mut svc = layer.layer(StatefulSlowFirst::new());

    let _ = tokio::time::timeout(Duration::from_secs(2), svc.ready().await.unwrap().call(()))
        .await
        .expect("hedge call hung -- likely attempt-readiness regression");
}

#[tokio::test]
async fn hedge_parallel_mode_drives_readied_instance_on_attempts() {
    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .build();
    let mut svc = layer.layer(StatefulSlowFirst::new());

    let _ = tokio::time::timeout(Duration::from_secs(2), svc.ready().await.unwrap().call(()))
        .await
        .expect("hedge call hung -- likely attempt-readiness regression");
}

#[tokio::test]
async fn first_success_drops_losers_before_emitting_terminal_event() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let terminal_snapshot = Arc::new(Mutex::new(None));
    let snapshot_slot = Arc::clone(&terminal_snapshot);
    let listener_handle = handle.clone();

    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .on_event(FnListener::new(move |event: &HedgeEvent| {
            if matches!(
                event,
                HedgeEvent::PrimarySucceeded { .. } | HedgeEvent::HedgeSucceeded { .. }
            ) {
                *snapshot_slot
                    .lock()
                    .expect("terminal snapshot lock poisoned") = Some(listener_handle.snapshot());
            }
        }))
        .build();
    let mut service = layer.layer(probe);
    let mut call = Box::pin(
        ServiceExt::<&'static str>::ready(&mut service)
            .await
            .unwrap()
            .call("request"),
    );

    tokio::select! {
        _ = &mut call => panic!("controlled attempts completed without a permit"),
        () = wait_for_probe_calls(&handle, 3) => {}
    }

    controller.allow(1);
    assert_eq!(call.await.unwrap(), "request");

    let snapshot = handle.snapshot();
    assert_eq!(snapshot.calls, 3);
    assert_eq!(snapshot.completed, 1);
    assert_eq!(snapshot.cancelled, 2);
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.peak_in_flight, 3);
    assert_eq!(
        terminal_snapshot
            .lock()
            .expect("terminal snapshot lock poisoned")
            .as_ref(),
        Some(&snapshot),
        "losers must be dropped before the success event is emitted"
    );
    handle.assert_ready_contract();
    handle.assert_quiescent();
}

#[tokio::test]
async fn dropping_caller_future_cancels_every_attempt_without_terminal_event() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let terminal_events = Arc::new(AtomicUsize::new(0));
    let listener_events = Arc::clone(&terminal_events);

    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .on_event(FnListener::new(move |event: &HedgeEvent| {
            if matches!(
                event,
                HedgeEvent::PrimarySucceeded { .. }
                    | HedgeEvent::HedgeSucceeded { .. }
                    | HedgeEvent::AllFailed { .. }
            ) {
                listener_events.fetch_add(1, Ordering::SeqCst);
            }
        }))
        .build();
    let mut service = layer.layer(probe);
    let mut call = Box::pin(
        ServiceExt::<&'static str>::ready(&mut service)
            .await
            .unwrap()
            .call("request"),
    );

    tokio::select! {
        _ = &mut call => panic!("controlled attempts completed without a permit"),
        () = wait_for_probe_calls(&handle, 3) => {}
    }
    drop(call);

    let snapshot = handle.snapshot();
    assert_eq!(snapshot.calls, 3);
    assert_eq!(snapshot.completed, 0);
    assert_eq!(snapshot.cancelled, 3);
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(terminal_events.load(Ordering::SeqCst), 0);

    // Permits cannot revive detached work after the caller has cancelled.
    controller.allow(3);
    tokio::task::yield_now().await;
    assert_eq!(handle.snapshot(), snapshot);
    handle.assert_ready_contract();
    handle.assert_quiescent();
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClassifiedRequest {
    idempotent: bool,
}

#[tokio::test]
async fn ineligible_request_executes_once() {
    let (controlled, controller) = ControlledService::new(true);
    let probe = ServiceProbe::new(controlled);
    let handle = probe.handle();
    let hedge_starts = Arc::new(AtomicUsize::new(0));
    let listener_starts = Arc::clone(&hedge_starts);

    let layer = HedgeLayer::builder()
        .eligible_if(|request: &ClassifiedRequest| request.idempotent)
        .no_delay()
        .max_hedged_attempts(3)
        .on_event(FnListener::new(move |event: &HedgeEvent| {
            if matches!(event, HedgeEvent::HedgeStarted { .. }) {
                listener_starts.fetch_add(1, Ordering::SeqCst);
            }
        }))
        .build();
    let mut service = layer.layer(probe);
    let request = ClassifiedRequest { idempotent: false };
    let mut call = Box::pin(service.ready().await.unwrap().call(request.clone()));

    tokio::select! {
        _ = &mut call => panic!("controlled primary completed without a permit"),
        () = wait_for_probe_calls(&handle, 1) => {}
    }
    assert_eq!(handle.snapshot().calls, 1);
    assert_eq!(hedge_starts.load(Ordering::SeqCst), 0);

    controller.allow(1);
    assert_eq!(call.await.unwrap(), request);
    let snapshot = handle.snapshot();
    assert_eq!(snapshot.calls, 1);
    assert_eq!(snapshot.completed, 1);
    assert_eq!(snapshot.cancelled, 0);
    assert_eq!(snapshot.peak_in_flight, 1);
    handle.assert_ready_contract();
    handle.assert_quiescent();
}

#[tokio::test]
async fn all_failed_waits_for_every_attempt_and_returns_primary_error() {
    let (inner, state) = OrderedErrorService::new();
    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(inner);
    let mut call = Box::pin(service.ready().await.unwrap().call(()));

    tokio::select! {
        result = &mut call => panic!("controlled errors completed without permits: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(1), async {
            while state.calls.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        }) => result.expect("both error attempts must start"),
    }

    state.hedge.add_permits(1);
    tokio::select! {
        biased;
        result = &mut call => panic!("hedge error returned before primary completed: {result:?}"),
        _ = tokio::task::yield_now() => {}
    }

    state.primary.add_permits(1);
    assert!(matches!(
        call.await,
        Err(HedgeError::AllAttemptsFailed(AttemptError::Primary))
    ));
}
