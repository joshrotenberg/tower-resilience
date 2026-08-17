//! Test helpers for tower-resilience layer crates.
//!
//! This module is gated behind the `testing` feature and is intended for use
//! in `[dev-dependencies]` only. It exposes inner-service probes that exercise
//! tower contract requirements that synthetic `service_fn`-based test doubles
//! cannot reach.
//!
//! # Why
//!
//! Every layer in this workspace implements [`tower::Service`]. The trait has
//! a contract that, if violated, lets compliant downstream middleware panic at
//! runtime:
//!
//! > Implementations are permitted to panic if `call` is invoked without
//! > obtaining `Poll::Ready(Ok(()))` from `poll_ready`.
//!
//! Tests that wrap a layer around `tower::service_fn` or a `MockService` style
//! probe never exercise this -- those inners have no-op `poll_ready` and no
//! per-instance readiness state, so the contract violation is invisible. The
//! The probes in this module deliberately reset per-instance readiness on
//! `Clone`, mirroring how `tower::limit::ConcurrencyLimit`, `Buffer`,
//! `LoadShed`, and other stateful tower middleware behave in production.
//! `StatefulInner` remains the minimal panic-on-violation probe;
//! `ServiceProbe` and `ControlledService` add observations and deterministic
//! control for multi-attempt and concurrent paths.
//!
//! # Example
//!
//! ```ignore
//! use tower::{Layer, Service, ServiceExt};
//! use tower_resilience_core::testing::StatefulInner;
//!
//! #[tokio::test]
//! async fn my_layer_drives_readied_instance() {
//!     let layer = MyLayer::builder().build();
//!     let mut svc = tower::ServiceBuilder::new()
//!         .layer(layer)
//!         .service(StatefulInner::new());
//!
//!     for _ in 0..3 {
//!         let _ = svc.ready().await.unwrap().call(()).await;
//!     }
//! }
//! ```
//!
//! See `CONTRIBUTING.md` for the full `Service` impl checklist.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// A snapshot of the observations made by a [`ServiceProbe`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProbeSnapshot {
    /// Number of times the probe was cloned.
    pub clones: usize,
    /// Number of calls to [`tower::Service::poll_ready`].
    pub readiness_polls: usize,
    /// Number of readiness polls that returned `Ready(Ok(()))`.
    pub readiness_successes: usize,
    /// Number of readiness polls that returned `Pending`.
    pub readiness_pending: usize,
    /// Number of readiness polls that returned `Ready(Err(_))`.
    pub readiness_errors: usize,
    /// Number of calls to [`tower::Service::call`].
    pub calls: usize,
    /// Calls made without readiness obtained on the same service instance.
    pub readiness_violations: usize,
    /// Futures currently returned by `call` and not yet completed or dropped.
    pub in_flight: usize,
    /// Maximum number of call futures simultaneously in flight.
    pub peak_in_flight: usize,
    /// Call futures that completed.
    pub completed: usize,
    /// Call futures dropped before completion.
    pub cancelled: usize,
}

#[derive(Debug, Default)]
struct ProbeState {
    clones: AtomicUsize,
    readiness_polls: AtomicUsize,
    readiness_successes: AtomicUsize,
    readiness_pending: AtomicUsize,
    readiness_errors: AtomicUsize,
    calls: AtomicUsize,
    readiness_violations: AtomicUsize,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
    completed: AtomicUsize,
    cancelled: AtomicUsize,
}

/// Shared observations for a [`ServiceProbe`] and all of its clones.
#[derive(Clone, Debug, Default)]
pub struct ProbeHandle {
    state: Arc<ProbeState>,
}

impl ProbeHandle {
    /// Loads the current value of every observation counter.
    ///
    /// Each counter is atomic, but concurrent activity may advance some
    /// counters while the snapshot is being assembled. Quiesce the service
    /// before asserting relationships between counters.
    pub fn snapshot(&self) -> ProbeSnapshot {
        ProbeSnapshot {
            clones: self.state.clones.load(Ordering::SeqCst),
            readiness_polls: self.state.readiness_polls.load(Ordering::SeqCst),
            readiness_successes: self.state.readiness_successes.load(Ordering::SeqCst),
            readiness_pending: self.state.readiness_pending.load(Ordering::SeqCst),
            readiness_errors: self.state.readiness_errors.load(Ordering::SeqCst),
            calls: self.state.calls.load(Ordering::SeqCst),
            readiness_violations: self.state.readiness_violations.load(Ordering::SeqCst),
            in_flight: self.state.in_flight.load(Ordering::SeqCst),
            peak_in_flight: self.state.peak_in_flight.load(Ordering::SeqCst),
            completed: self.state.completed.load(Ordering::SeqCst),
            cancelled: self.state.cancelled.load(Ordering::SeqCst),
        }
    }

    /// Panics if any call bypassed readiness on its exact service instance.
    ///
    /// Keeping the assertion on the handle allows a test to observe all
    /// attempts before failing, including attempts running in spawned tasks.
    pub fn assert_ready_contract(&self) {
        let snapshot = self.snapshot();
        assert_eq!(
            snapshot.readiness_violations, 0,
            "{} Service::call invocation(s) occurred without prior poll_ready on the same instance",
            snapshot.readiness_violations
        );
    }

    /// Panics if any returned call future is still alive.
    pub fn assert_quiescent(&self) {
        let snapshot = self.snapshot();
        assert_eq!(
            snapshot.in_flight, 0,
            "{} call future(s) still in flight",
            snapshot.in_flight
        );
    }
}

/// A composable [`tower::Service`] contract probe.
///
/// The probe delegates responses and errors unchanged while recording
/// readiness, clone, concurrency, completion, and cancellation behavior. Its
/// [`Clone`] implementation deliberately resets per-instance readiness. This
/// makes it suitable for retry, reconnect, hedge, and router paths that create
/// additional service instances internally.
///
/// Unlike [`StatefulInner`], a readiness violation is recorded rather than
/// panicking immediately. Call [`ProbeHandle::assert_ready_contract`] after
/// the operation so violations in spawned attempts are reported reliably.
pub struct ServiceProbe<S> {
    inner: S,
    ready: bool,
    handle: ProbeHandle,
}

impl<S> ServiceProbe<S> {
    /// Wraps `inner` in a new probe.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            ready: false,
            handle: ProbeHandle::default(),
        }
    }

    /// Returns a handle shared with this probe and all future clones.
    pub fn handle(&self) -> ProbeHandle {
        self.handle.clone()
    }

    /// Returns the wrapped service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> Clone for ServiceProbe<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        self.handle.state.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: self.inner.clone(),
            ready: false,
            handle: self.handle.clone(),
        }
    }
}

impl<S, Request> tower::Service<Request> for ServiceProbe<S>
where
    S: tower::Service<Request>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ProbeFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.handle
            .state
            .readiness_polls
            .fetch_add(1, Ordering::SeqCst);

        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => {
                self.ready = true;
                self.handle
                    .state
                    .readiness_successes
                    .fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                self.ready = false;
                self.handle
                    .state
                    .readiness_errors
                    .fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Err(error))
            }
            Poll::Pending => {
                self.handle
                    .state
                    .readiness_pending
                    .fetch_add(1, Ordering::SeqCst);
                Poll::Pending
            }
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        self.handle.state.calls.fetch_add(1, Ordering::SeqCst);
        if !std::mem::take(&mut self.ready) {
            self.handle
                .state
                .readiness_violations
                .fetch_add(1, Ordering::SeqCst);
        }

        let future = self.inner.call(request);
        let in_flight = self.handle.state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.handle
            .state
            .peak_in_flight
            .fetch_max(in_flight, Ordering::SeqCst);

        ProbeFuture {
            inner: Box::pin(future),
            state: Arc::clone(&self.handle.state),
            finished: false,
        }
    }
}

/// A call future returned by [`ServiceProbe`].
pub struct ProbeFuture<F> {
    inner: Pin<Box<F>>,
    state: Arc<ProbeState>,
    finished: bool,
}

impl<F> Future for ProbeFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(output) => {
                this.finished = true;
                this.state.completed.fetch_add(1, Ordering::SeqCst);
                this.state.in_flight.fetch_sub(1, Ordering::SeqCst);
                Poll::Ready(output)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<F> Drop for ProbeFuture<F> {
    fn drop(&mut self) {
        if !self.finished {
            self.state.cancelled.fetch_add(1, Ordering::SeqCst);
            self.state.in_flight.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Error returned after a [`ControlledService`] is closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlledServiceClosed;

impl std::fmt::Display for ControlledServiceClosed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("controlled service is closed")
    }
}

impl std::error::Error for ControlledServiceClosed {}

#[derive(Debug)]
struct ControlledState {
    ready: AtomicBool,
    closed: AtomicBool,
    calls: AtomicUsize,
    permits: Arc<tokio::sync::Semaphore>,
    readiness_wakers: Mutex<Vec<Waker>>,
}

/// Controller for a [`ControlledService`].
#[derive(Clone, Debug)]
pub struct ControlledHandle {
    state: Arc<ControlledState>,
}

impl ControlledHandle {
    /// Changes readiness and wakes all registered readiness waiters when open.
    pub fn set_ready(&self, ready: bool) {
        self.state.ready.store(ready, Ordering::SeqCst);
        if ready {
            self.wake_readiness_waiters();
        }
    }

    /// Allows `count` call futures to complete.
    pub fn allow(&self, count: usize) {
        self.state.permits.add_permits(count);
    }

    /// Permanently closes readiness and all pending call futures.
    pub fn close(&self) {
        self.state.closed.store(true, Ordering::SeqCst);
        self.state.permits.close();
        self.wake_readiness_waiters();
    }

    /// Returns how many requests reached the controlled service.
    pub fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }

    fn wake_readiness_waiters(&self) {
        let waiters = std::mem::take(
            &mut *self
                .state
                .readiness_wakers
                .lock()
                .expect("controlled service readiness lock poisoned"),
        );
        for waker in waiters {
            waker.wake();
        }
    }
}

/// A deterministic service with externally controlled readiness and completion.
///
/// `poll_ready` remains pending until [`ControlledHandle::set_ready`] is called.
/// Every accepted call then waits for one permit from [`ControlledHandle::allow`]
/// and echoes its request unchanged. Compose this with [`ServiceProbe`] to test
/// wakeups, clone-heavy admission, cancellation, and thundering-herd behavior
/// without sleeps.
#[derive(Clone, Debug)]
pub struct ControlledService {
    handle: ControlledHandle,
}

impl ControlledService {
    /// Creates a service/controller pair with the requested initial readiness.
    pub fn new(initially_ready: bool) -> (Self, ControlledHandle) {
        let handle = ControlledHandle {
            state: Arc::new(ControlledState {
                ready: AtomicBool::new(initially_ready),
                closed: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
                permits: Arc::new(tokio::sync::Semaphore::new(0)),
                readiness_wakers: Mutex::new(Vec::new()),
            }),
        };
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl<Request> tower::Service<Request> for ControlledService
where
    Request: Send + 'static,
{
    type Response = Request;
    type Error = ControlledServiceClosed;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.handle.state.closed.load(Ordering::SeqCst) {
            return Poll::Ready(Err(ControlledServiceClosed));
        }
        if self.handle.state.ready.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }

        let mut waiters = self
            .handle
            .state
            .readiness_wakers
            .lock()
            .expect("controlled service readiness lock poisoned");
        if self.handle.state.closed.load(Ordering::SeqCst) {
            return Poll::Ready(Err(ControlledServiceClosed));
        }
        if self.handle.state.ready.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }
        if !waiters.iter().any(|waker| waker.will_wake(cx.waker())) {
            waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }

    fn call(&mut self, request: Request) -> Self::Future {
        self.handle.state.calls.fetch_add(1, Ordering::SeqCst);
        let permits = Arc::clone(&self.handle.state.permits);
        Box::pin(async move {
            let permit = permits
                .acquire_owned()
                .await
                .map_err(|_| ControlledServiceClosed)?;
            permit.forget();
            Ok(request)
        })
    }
}

/// An inner [`tower::Service`] whose [`Clone`] resets readiness state.
///
/// `poll_ready` flips `ready: true`; `call` asserts `ready` is set, then flips
/// it back to `false`. Crucially, [`Clone`] produces an instance with
/// `ready: false`, mirroring the documented behavior of stateful tower
/// middleware like `tower::limit::ConcurrencyLimit`.
///
/// Wrap a layer around this and drive multiple `ready().await; call(...)`
/// cycles. If the layer moves a fresh `self.inner.clone()` into its returned
/// future (rather than the readied original via `std::mem::replace`), the
/// `call` assertion panics with a message naming #286.
pub struct StatefulInner {
    ready: bool,
}

impl StatefulInner {
    /// Constructs an un-ready probe.
    pub fn new() -> Self {
        Self { ready: false }
    }
}

impl Default for StatefulInner {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StatefulInner {
    fn clone(&self) -> Self {
        // Mirrors `Clone for ConcurrencyLimit` / `Buffer`: the new instance
        // does not inherit readiness from the original.
        Self { ready: false }
    }
}

impl tower::Service<()> for StatefulInner {
    type Response = ();
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#286)"
        );
        // The contract: call consumes readiness. Next call must re-poll.
        self.ready = false;
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use tower::{service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn records_and_rejects_a_readiness_violation() {
        let mut probe = ServiceProbe::new(service_fn(|request: usize| async move {
            Ok::<_, Infallible>(request)
        }));
        let handle = probe.handle();

        assert_eq!(probe.call(7).await.unwrap(), 7);
        assert_eq!(handle.snapshot().readiness_violations, 1);

        let assertion = std::panic::catch_unwind(|| handle.assert_ready_contract());
        assert!(
            assertion.is_err(),
            "the nonconforming call must be rejected"
        );
    }

    #[tokio::test]
    async fn clone_does_not_inherit_readiness() {
        let mut probe = ServiceProbe::new(service_fn(|(): ()| async { Ok::<_, Infallible>(()) }));
        probe.ready().await.unwrap();
        let mut clone = probe.clone();
        let handle = clone.handle();

        clone.call(()).await.unwrap();

        let snapshot = handle.snapshot();
        assert_eq!(snapshot.clones, 1);
        assert_eq!(snapshot.readiness_violations, 1);
    }

    #[tokio::test]
    async fn tracks_completion_cancellation_and_peak_in_flight() {
        let (controlled, controller) = ControlledService::new(true);
        let mut probe = ServiceProbe::new(controlled);
        let handle = probe.handle();

        let first = ServiceExt::<&'static str>::ready(&mut probe)
            .await
            .unwrap()
            .call("first");
        let cancelled = ServiceExt::<&'static str>::ready(&mut probe)
            .await
            .unwrap()
            .call("cancelled");
        drop(cancelled);
        let second = ServiceExt::<&'static str>::ready(&mut probe)
            .await
            .unwrap()
            .call("second");

        controller.allow(2);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap(), "first");
        assert_eq!(second.unwrap(), "second");

        let snapshot = handle.snapshot();
        assert_eq!(snapshot.calls, 3);
        assert_eq!(snapshot.peak_in_flight, 2);
        assert_eq!(snapshot.completed, 2);
        assert_eq!(snapshot.cancelled, 1);
        handle.assert_ready_contract();
        handle.assert_quiescent();
    }

    #[tokio::test]
    async fn controlled_readiness_registers_and_wakes() {
        let (controlled, controller) = ControlledService::new(false);
        let probe = ServiceProbe::new(controlled);
        let handle = probe.handle();
        let task = tokio::spawn(async move {
            let mut probe = probe;
            ServiceExt::<()>::ready(&mut probe).await.unwrap();
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while handle.snapshot().readiness_pending == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("readiness was never polled");

        controller.set_ready(true);
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("registered readiness waiter was not woken")
            .unwrap();
        assert!(handle.snapshot().readiness_successes >= 1);
    }

    #[tokio::test]
    async fn preserves_responses_and_readiness_errors() {
        let (controlled, controller) = ControlledService::new(true);
        let mut probe = ServiceProbe::new(controlled);
        let handle = probe.handle();

        let response = ServiceExt::<String>::ready(&mut probe)
            .await
            .unwrap()
            .call(String::from("sentinel"));
        controller.allow(1);
        assert_eq!(response.await.unwrap(), "sentinel");

        controller.close();
        assert!(matches!(
            ServiceExt::<String>::ready(&mut probe).await,
            Err(ControlledServiceClosed)
        ));
        assert_eq!(handle.snapshot().readiness_errors, 1);
    }

    #[tokio::test]
    async fn preserves_call_errors() {
        let mut probe = ServiceProbe::new(service_fn(|(): ()| async {
            Err::<(), _>("sentinel call error")
        }));
        let handle = probe.handle();

        let error = probe.ready().await.unwrap().call(()).await.unwrap_err();

        assert_eq!(error, "sentinel call error");
        assert_eq!(handle.snapshot().completed, 1);
        handle.assert_ready_contract();
        handle.assert_quiescent();
    }
}
