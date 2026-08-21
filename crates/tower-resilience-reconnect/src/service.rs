use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll},
};

use tower::Service;

use crate::{config::ReconnectConfig, state::ReconnectState};

#[cfg(feature = "tracing")]
use tracing::{debug, warn};

#[cfg(feature = "metrics")]
use metrics::{counter, gauge};

/// A Tower service that owns a service factory and replaces failed connections.
///
/// `M` is a `MakeService`-style factory: a `Service<Target>` whose response is
/// another `Service`. All clones coordinate through one shared connection. A
/// classified connection failure invalidates that connection generation, and
/// the next retry is issued only after the factory has produced a fresh service.
pub struct ReconnectService<M, Target>
where
    M: Service<Target>,
{
    shared: Arc<Mutex<Shared<M, Target>>>,
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
    ready: Option<(u64, M::Response)>,
    readiness_attempts: u32,
}

impl<M, Target> ReconnectService<M, Target>
where
    M: Service<Target>,
{
    /// Creates a reconnecting service from a factory, target, and configuration.
    pub fn new(factory: M, target: Target, config: ReconnectConfig) -> Self {
        Self::from_parts(factory, target, Arc::new(config), ReconnectState::new())
    }

    pub(crate) fn from_parts(
        factory: M,
        target: Target,
        config: Arc<ReconnectConfig>,
        state: ReconnectState,
    ) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared::new(factory, target))),
            config,
            state,
            ready: None,
            readiness_attempts: 0,
        }
    }

    /// Returns the shared reconnection state.
    pub fn state(&self) -> &ReconnectState {
        &self.state
    }

    /// Returns the reconnection configuration.
    pub fn config(&self) -> &ReconnectConfig {
        &self.config
    }
}

impl<M, Target> Clone for ReconnectService<M, Target>
where
    M: Service<Target>,
{
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            config: Arc::clone(&self.config),
            state: self.state.clone(),
            // Readiness belongs to the receiver that observed it. A clone must
            // acquire and poll its own service instance.
            ready: None,
            readiness_attempts: 0,
        }
    }
}

impl<M, Target, Request> Service<Request> for ReconnectService<M, Target>
where
    M: Service<Target>,
    M::Response: Service<Request> + Clone,
    M::Error: std::error::Error + Send + Sync + 'static,
    <M::Response as Service<Request>>::Error: std::error::Error + Send + Sync + 'static,
    Target: Clone,
    Request: Clone,
{
    type Response = <M::Response as Service<Request>>::Response;
    type Error = ReconnectError<M::Error, <M::Response as Service<Request>>::Error>;
    type Future = ReconnectFuture<M, Target, Request>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        loop {
            if let Some((generation, service)) = self.ready.as_mut() {
                if !is_current_generation(&self.shared, *generation) {
                    self.ready = None;
                    continue;
                }

                match service.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        self.readiness_attempts = 0;
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => {
                        let generation = *generation;
                        self.ready = None;

                        if !self.config.should_reconnect(&error) {
                            return Poll::Ready(Err(ReconnectError::ServiceError(error)));
                        }

                        self.readiness_attempts += 1;
                        if exceeded(self.config.max_attempts, self.readiness_attempts) {
                            return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                                attempts: self.readiness_attempts,
                                error: Box::new(error),
                            }));
                        }

                        let can_reconnect = invalidate(
                            &self.shared,
                            generation,
                            self.readiness_attempts,
                            &self.config,
                            &self.state,
                        );
                        if !can_reconnect {
                            return Poll::Ready(Err(ReconnectError::ConnectionFailed(error)));
                        }
                    }
                }
            } else {
                match poll_connection(&self.shared, &self.config, &self.state, cx) {
                    Poll::Ready(Ok(connection)) => self.ready = Some(connection),
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => return Poll::Pending,
                }
            }
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let (generation, mut service) = self
            .ready
            .take()
            .expect("ReconnectService::call invoked without poll_ready");
        let future = service.call(request.clone());

        ReconnectFuture {
            shared: Arc::clone(&self.shared),
            config: Arc::clone(&self.config),
            state: self.state.clone(),
            request,
            attempts: 0,
            phase: ResponsePhase::Calling {
                generation,
                future: Box::pin(future),
            },
        }
    }
}

struct Shared<M, Target>
where
    M: Service<Target>,
{
    factory: M,
    target: Target,
    generation: u64,
    factory_attempts: u32,
    phase: ConnectionPhase<M::Future, M::Response>,
}

impl<M, Target> Shared<M, Target>
where
    M: Service<Target>,
{
    fn new(factory: M, target: Target) -> Self {
        Self {
            factory,
            target,
            generation: 0,
            factory_attempts: 0,
            phase: ConnectionPhase::Idle,
        }
    }
}

enum ConnectionPhase<F, S> {
    Idle,
    Sleeping(Pin<Box<tokio::time::Sleep>>),
    Connecting(Pin<Box<F>>),
    Connected(S),
}

type ConnectionPoll<M, Target, Request> = Poll<
    Result<
        (u64, <M as Service<Target>>::Response),
        ReconnectError<
            <M as Service<Target>>::Error,
            <<M as Service<Target>>::Response as Service<Request>>::Error,
        >,
    >,
>;

/// Future returned by [`ReconnectService`].
pub struct ReconnectFuture<M, Target, Request>
where
    M: Service<Target>,
    M::Response: Service<Request>,
{
    shared: Arc<Mutex<Shared<M, Target>>>,
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
    request: Request,
    attempts: u32,
    phase: ResponsePhase<<M::Response as Service<Request>>::Future, M::Response>,
}

// Inner call/factory futures remain pinned in `Pin<Box<_>>`. The remaining
// fields are ordinary state and are never exposed as pinned projections.
impl<M, Target, Request> Unpin for ReconnectFuture<M, Target, Request>
where
    M: Service<Target>,
    M::Response: Service<Request>,
{
}

enum ResponsePhase<F, S> {
    Calling {
        generation: u64,
        future: Pin<Box<F>>,
    },
    Reconnecting,
    Readying {
        generation: u64,
        service: S,
    },
    Done,
}

impl<M, Target, Request> Future for ReconnectFuture<M, Target, Request>
where
    M: Service<Target>,
    M::Response: Service<Request> + Clone,
    M::Error: std::error::Error + Send + Sync + 'static,
    <M::Response as Service<Request>>::Error: std::error::Error + Send + Sync + 'static,
    Target: Clone,
    Request: Clone,
{
    type Output = Result<
        <M::Response as Service<Request>>::Response,
        ReconnectError<M::Error, <M::Response as Service<Request>>::Error>,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();

        loop {
            match &mut this.phase {
                ResponsePhase::Calling { generation, future } => match future.as_mut().poll(cx) {
                    Poll::Ready(Ok(response)) => {
                        this.phase = ResponsePhase::Done;
                        return Poll::Ready(Ok(response));
                    }
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => {
                        if !this.config.should_reconnect(&error) {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::ServiceError(error)));
                        }

                        this.attempts += 1;
                        if exceeded(this.config.max_attempts, this.attempts) {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                                attempts: this.attempts,
                                error: Box::new(error),
                            }));
                        }

                        let can_reconnect = invalidate(
                            &this.shared,
                            *generation,
                            this.attempts,
                            &this.config,
                            &this.state,
                        );
                        if !can_reconnect {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::ConnectionFailed(error)));
                        }

                        if !this.config.retry_on_reconnect {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::ConnectionFailedNoRetry(
                                error,
                            )));
                        }

                        this.phase = ResponsePhase::Reconnecting;
                    }
                },
                ResponsePhase::Reconnecting => {
                    match poll_connection(&this.shared, &this.config, &this.state, cx) {
                        Poll::Ready(Ok((generation, service))) => {
                            this.phase = ResponsePhase::Readying {
                                generation,
                                service,
                            };
                        }
                        Poll::Ready(Err(error)) => {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(error));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                ResponsePhase::Readying {
                    generation,
                    service,
                } => match service.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        let future = service.call(this.request.clone());
                        this.phase = ResponsePhase::Calling {
                            generation: *generation,
                            future: Box::pin(future),
                        };
                    }
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => {
                        if !this.config.should_reconnect(&error) {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::ServiceError(error)));
                        }

                        this.attempts += 1;
                        if exceeded(this.config.max_attempts, this.attempts) {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                                attempts: this.attempts,
                                error: Box::new(error),
                            }));
                        }

                        let can_reconnect = invalidate(
                            &this.shared,
                            *generation,
                            this.attempts,
                            &this.config,
                            &this.state,
                        );
                        if !can_reconnect {
                            this.phase = ResponsePhase::Done;
                            return Poll::Ready(Err(ReconnectError::ConnectionFailed(error)));
                        }
                        this.phase = ResponsePhase::Reconnecting;
                    }
                },
                ResponsePhase::Done => panic!("ReconnectFuture polled after completion"),
            }
        }
    }
}

fn poll_connection<M, Target, Request>(
    shared: &Arc<Mutex<Shared<M, Target>>>,
    config: &ReconnectConfig,
    state: &ReconnectState,
    cx: &mut Context<'_>,
) -> ConnectionPoll<M, Target, Request>
where
    M: Service<Target>,
    M::Response: Service<Request> + Clone,
    M::Error: std::error::Error + Send + Sync + 'static,
    <M::Response as Service<Request>>::Error: std::error::Error + Send + Sync + 'static,
    Target: Clone,
{
    let mut shared = lock(shared);

    loop {
        let generation = shared.generation;
        match &mut shared.phase {
            ConnectionPhase::Idle => match shared.factory.poll_ready(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => {
                    let target = shared.target.clone();
                    let future = shared.factory.call(target);
                    shared.phase = ConnectionPhase::Connecting(Box::pin(future));
                    mark_reconnecting(config, state, None);
                    continue;
                }
                Poll::Ready(Err(error)) => {
                    if !schedule_factory_retry(&mut shared, config, state) {
                        return Poll::Ready(Err(ReconnectError::FactoryError(error)));
                    }
                    if exceeded(config.max_attempts, shared.factory_attempts) {
                        return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                            attempts: shared.factory_attempts,
                            error: Box::new(error),
                        }));
                    }
                }
            },
            ConnectionPhase::Sleeping(sleep) => match sleep.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => shared.phase = ConnectionPhase::Idle,
            },
            ConnectionPhase::Connecting(future) => match future.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(service)) => {
                    shared.generation = shared.generation.wrapping_add(1);
                    shared.factory_attempts = 0;
                    shared.phase = ConnectionPhase::Connected(service);
                    mark_connected(config, state);
                }
                Poll::Ready(Err(error)) => {
                    if !schedule_factory_retry(&mut shared, config, state) {
                        return Poll::Ready(Err(ReconnectError::FactoryError(error)));
                    }
                    if exceeded(config.max_attempts, shared.factory_attempts) {
                        return Poll::Ready(Err(ReconnectError::MaxAttemptsExceeded {
                            attempts: shared.factory_attempts,
                            error: Box::new(error),
                        }));
                    }
                }
            },
            ConnectionPhase::Connected(service) => {
                return Poll::Ready(Ok((generation, service.clone())));
            }
        }
    }
}

fn schedule_factory_retry<M, Target>(
    shared: &mut Shared<M, Target>,
    config: &ReconnectConfig,
    state: &ReconnectState,
) -> bool
where
    M: Service<Target>,
{
    shared.factory_attempts += 1;
    mark_disconnected(config, state);
    state.increment_attempts();

    #[cfg(feature = "metrics")]
    counter!("reconnect_attempts_total").increment(1);

    #[cfg(feature = "tracing")]
    warn!(
        attempt = shared.factory_attempts,
        "Reconnection attempt scheduled"
    );

    let Some(delay) = config
        .policy
        .delay_for_attempt(shared.factory_attempts as usize)
    else {
        shared.phase = ConnectionPhase::Idle;
        return false;
    };

    mark_reconnecting(config, state, Some(shared.factory_attempts));
    shared.phase = ConnectionPhase::Sleeping(Box::pin(tokio::time::sleep(delay)));
    true
}

fn invalidate<M, Target>(
    shared: &Arc<Mutex<Shared<M, Target>>>,
    generation: u64,
    attempt: u32,
    config: &ReconnectConfig,
    state: &ReconnectState,
) -> bool
where
    M: Service<Target>,
{
    let mut shared = lock(shared);

    // Several in-flight requests can observe the same broken generation. Only
    // the first failure replaces it; the others join that reconnect attempt.
    if shared.generation != generation || !matches!(shared.phase, ConnectionPhase::Connected(_)) {
        return true;
    }

    mark_disconnected(config, state);
    state.increment_attempts();

    #[cfg(feature = "metrics")]
    counter!("reconnect_attempts_total").increment(1);

    #[cfg(feature = "tracing")]
    warn!(attempt, "Reconnection attempt scheduled");

    let Some(delay) = config.policy.delay_for_attempt(attempt as usize) else {
        shared.phase = ConnectionPhase::Idle;
        shared.generation = shared.generation.wrapping_add(1);
        return false;
    };

    mark_reconnecting(config, state, Some(attempt));
    shared.phase = ConnectionPhase::Sleeping(Box::pin(tokio::time::sleep(delay)));
    shared.generation = shared.generation.wrapping_add(1);
    true
}

fn is_current_generation<M, Target>(shared: &Arc<Mutex<Shared<M, Target>>>, generation: u64) -> bool
where
    M: Service<Target>,
{
    let shared = lock(shared);
    shared.generation == generation && matches!(shared.phase, ConnectionPhase::Connected(_))
}

fn lock<M, Target>(shared: &Arc<Mutex<Shared<M, Target>>>) -> MutexGuard<'_, Shared<M, Target>>
where
    M: Service<Target>,
{
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn exceeded(max_attempts: Option<u32>, attempts: u32) -> bool {
    max_attempts.is_some_and(|max| attempts > max)
}

fn mark_disconnected(config: &ReconnectConfig, state: &ReconnectState) {
    let previous = state.state();
    state.mark_disconnected();
    notify_state_change(
        config,
        previous,
        crate::state::ConnectionState::Disconnected,
    );
}

fn mark_reconnecting(config: &ReconnectConfig, state: &ReconnectState, attempt: Option<u32>) {
    let previous = state.state();
    state.mark_reconnecting();
    notify_state_change(
        config,
        previous,
        crate::state::ConnectionState::Reconnecting,
    );

    #[cfg(feature = "tracing")]
    if let (Some(attempt), Some(callback)) = (attempt, config.on_reconnect.as_ref()) {
        callback(attempt);
    }

    #[cfg(not(feature = "tracing"))]
    let _ = attempt;
}

fn mark_connected(config: &ReconnectConfig, state: &ReconnectState) {
    let previous = state.state();
    state.mark_connected();
    notify_state_change(config, previous, crate::state::ConnectionState::Connected);
}

fn notify_state_change(
    config: &ReconnectConfig,
    previous: crate::state::ConnectionState,
    current: crate::state::ConnectionState,
) {
    if previous == current {
        return;
    }

    #[cfg(feature = "tracing")]
    debug!(from = ?previous, to = ?current, "Reconnect state transition");

    #[cfg(feature = "metrics")]
    {
        counter!(
            "reconnect_transitions_total",
            "from" => format!("{previous:?}"),
            "to" => format!("{current:?}")
        )
        .increment(1);
        gauge!("reconnect_state", "state" => format!("{current:?}")).set(1.0);
    }

    #[cfg(feature = "tracing")]
    if let Some(callback) = config.on_state_change.as_ref() {
        callback(previous, current);
    }

    #[cfg(not(any(feature = "tracing", feature = "metrics")))]
    let _ = config;

    #[cfg(all(feature = "metrics", not(feature = "tracing")))]
    let _ = config;
}

/// Errors that can occur during reconnection.
///
/// Factory and service errors remain available through their typed variants.
/// They are intentionally not exposed through [`std::error::Error::source`] so
/// boxed Tower errors can be wrapped directly. The boxed terminal cause in
/// [`ReconnectError::MaxAttemptsExceeded`] remains available as a source.
#[derive(Debug)]
pub enum ReconnectError<MakeError, ServiceError> {
    /// The maximum number of reconnection attempts was exceeded.
    MaxAttemptsExceeded {
        /// The number of attempts made.
        attempts: u32,
        /// The last error encountered.
        error: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The service factory failed to construct a connection.
    FactoryError(MakeError),
    /// Reconnection was disabled by the configured policy.
    ConnectionFailed(ServiceError),
    /// The connection failed and the original request was not retried.
    ConnectionFailedNoRetry(ServiceError),
    /// The connected service returned a non-reconnectable error.
    ServiceError(ServiceError),
}

impl<M, S> std::fmt::Display for ReconnectError<M, S>
where
    M: std::fmt::Display,
    S: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxAttemptsExceeded { attempts, error } => {
                write!(
                    f,
                    "max reconnection attempts ({attempts}) exceeded: {error}"
                )
            }
            Self::FactoryError(error) => write!(f, "connection factory failed: {error}"),
            Self::ConnectionFailed(error) => write!(f, "connection failed: {error}"),
            Self::ConnectionFailedNoRetry(error) => {
                write!(f, "connection failed (request not retried): {error}")
            }
            Self::ServiceError(error) => write!(f, "service error: {error}"),
        }
    }
}

impl<M, S> std::error::Error for ReconnectError<M, S>
where
    M: std::fmt::Debug + std::fmt::Display,
    S: std::fmt::Debug + std::fmt::Display,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MaxAttemptsExceeded { error, .. } => Some(error.as_ref()),
            Self::FactoryError(_)
            | Self::ConnectionFailed(_)
            | Self::ConnectionFailedNoRetry(_)
            | Self::ServiceError(_) => None,
        }
    }
}

#[cfg(all(test, feature = "metrics"))]
mod tests {
    use crate::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};
    use metrics::set_global_recorder;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, LazyLock};
    use std::time::Duration;
    use tower::{service_fn, Layer, Service, ServiceExt};

    #[derive(Clone)]
    struct Connection {
        ready: bool,
    }

    impl Service<()> for Connection {
        type Response = ();
        type Error = io::Error;
        type Future = std::future::Ready<Result<(), io::Error>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.ready = true;
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, (): ()) -> Self::Future {
            self.ready = false;
            std::future::ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn factory_retries_emit_attempt_and_transition_metrics() {
        static RECORDER: LazyLock<DebuggingRecorder> = LazyLock::new(DebuggingRecorder::default);
        let _ = set_global_recorder(&*RECORDER);

        let factory_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&factory_calls);
        let factory = service_fn(move |(): ()| {
            let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt < 2 {
                    Err(io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        "factory unavailable",
                    ))
                } else {
                    Ok(Connection { ready: false })
                }
            }
        });

        let config = ReconnectConfig::builder()
            .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
            .max_attempts(3)
            .build();
        let mut service = ReconnectLayer::new(config).layer(factory);

        service.ready().await.unwrap().call(()).await.unwrap();

        let snapshot = RECORDER.snapshotter().snapshot().into_vec();

        let attempts_recorded = snapshot.iter().any(|(key, _, _, value)| {
            key.key().name() == "reconnect_attempts_total"
                && matches!(value, DebugValue::Counter(v) if *v >= 1)
        });
        let transition_recorded = snapshot.iter().any(|(key, _, _, value)| {
            key.key().name() == "reconnect_transitions_total"
                && matches!(value, DebugValue::Counter(v) if *v >= 1)
                && key
                    .key()
                    .labels()
                    .any(|label| label.key() == "to" && label.value() == "Connected")
        });

        assert!(attempts_recorded, "expected reconnect_attempts_total > 0");
        assert!(
            transition_recorded,
            "expected a reconnect_transitions_total{{to=\"Connected\"}} entry"
        );
    }
}
