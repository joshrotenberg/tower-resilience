//! Generic Tower service backed fallback.

use crate::{FallbackError, FallbackEvent, HandlePredicate, HandleResponsePredicate};
use futures::future::{poll_fn, BoxFuture};
use futures::lock::Mutex;
use std::marker::PhantomData;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::{Layer, Service};
use tower_resilience_core::{EventListeners, FnListener};

#[cfg(feature = "metrics")]
use metrics::counter;

struct ServiceFallbackConfig<Res, E> {
    name: String,
    handle_predicate: Option<HandlePredicate<E>>,
    handle_response_predicate: Option<HandleResponsePredicate<Res>>,
    event_listeners: EventListeners<FallbackEvent>,
}

impl<Res, E> ServiceFallbackConfig<Res, E> {
    fn new() -> Self {
        Self {
            name: "fallback".to_string(),
            handle_predicate: None,
            handle_response_predicate: None,
            event_listeners: EventListeners::new(),
        }
    }
}

impl<Res, E> Clone for ServiceFallbackConfig<Res, E> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            handle_predicate: self.handle_predicate.clone(),
            handle_response_predicate: self.handle_response_predicate.clone(),
            event_listeners: self.event_listeners.clone(),
        }
    }
}

/// A layer that delegates failed primary requests to an arbitrary Tower
/// [`Service`].
///
/// Unlike [`FallbackLayer::service`](crate::FallbackLayer::service), this type
/// retains the concrete backup service type and drives its readiness before
/// every backup call. The backup does not need to implement `Clone`: clones of
/// this layer and the services it creates share one backup instance.
///
/// Outer readiness represents the primary service only. The backup is selected
/// after the primary result is known, so backup readiness is awaited inside the
/// returned call future. A backup readiness or call error is returned as
/// [`FallbackError::FallbackFailed`].
pub struct ServiceFallbackLayer<B, Req, Res, E> {
    backup: Arc<Mutex<B>>,
    config: ServiceFallbackConfig<Res, E>,
    _request: PhantomData<fn(Req)>,
}

impl<B, Req, Res, E> ServiceFallbackLayer<B, Req, Res, E> {
    /// Creates a service-backed fallback layer.
    pub fn new(backup: B) -> Self {
        Self {
            backup: Arc::new(Mutex::new(backup)),
            config: ServiceFallbackConfig::new(),
            _request: PhantomData,
        }
    }

    /// Sets the name used by events, tracing, and metrics.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Only delegates primary errors matching `predicate` to the backup.
    pub fn handle<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&E) -> bool + Send + Sync + 'static,
    {
        self.config.handle_predicate = Some(Arc::new(predicate));
        self
    }

    /// Delegates successful primary responses matching `predicate` to the
    /// backup service.
    pub fn handle_response<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&Res) -> bool + Send + Sync + 'static,
    {
        self.config.handle_response_predicate = Some(Arc::new(predicate));
        self
    }

    /// Adds an event listener.
    pub fn on_event<F>(mut self, listener: F) -> Self
    where
        F: Fn(&FallbackEvent) + Send + Sync + 'static,
    {
        self.config.event_listeners.add(FnListener::new(listener));
        self
    }
}

impl<B, Req, Res, E> Clone for ServiceFallbackLayer<B, Req, Res, E> {
    fn clone(&self) -> Self {
        Self {
            backup: Arc::clone(&self.backup),
            config: self.config.clone(),
            _request: PhantomData,
        }
    }
}

impl<S, B, Req, Res, E> Layer<S> for ServiceFallbackLayer<B, Req, Res, E> {
    type Service = ServiceFallback<S, B, Req, Res, E>;

    fn layer(&self, primary: S) -> Self::Service {
        ServiceFallback::new(
            primary,
            Arc::clone(&self.backup),
            Arc::new(self.config.clone()),
        )
    }
}

/// A primary Tower service paired with a generic backup Tower service.
pub struct ServiceFallback<S, B, Req, Res, E> {
    primary: S,
    backup: Arc<Mutex<B>>,
    config: Arc<ServiceFallbackConfig<Res, E>>,
    _request: PhantomData<fn(Req)>,
}

impl<S, B, Req, Res, E> ServiceFallback<S, B, Req, Res, E> {
    fn new(primary: S, backup: Arc<Mutex<B>>, config: Arc<ServiceFallbackConfig<Res, E>>) -> Self {
        crate::init_metrics();
        Self {
            primary,
            backup,
            config,
            _request: PhantomData,
        }
    }
}

impl<S, B, Req, Res, E> Clone for ServiceFallback<S, B, Req, Res, E>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            primary: self.primary.clone(),
            backup: Arc::clone(&self.backup),
            config: Arc::clone(&self.config),
            _request: PhantomData,
        }
    }
}

impl<S, B, Req, Res, E> Service<Req> for ServiceFallback<S, B, Req, Res, E>
where
    S: Service<Req, Response = Res, Error = E> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Service<Req, Response = Res> + Send + 'static,
    B::Future: Send + 'static,
    B::Error: Send + 'static,
    Req: Clone + Send + 'static,
    Res: Send + 'static,
    E: Send + 'static,
{
    type Response = Res;
    type Error = FallbackError<E, B::Error>;
    type Future = BoxFuture<'static, Result<Res, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.primary.poll_ready(cx).map_err(FallbackError::Inner)
    }

    fn call(&mut self, request: Req) -> Self::Future {
        // Preserve the exact primary instance made ready by poll_ready. The
        // request must be cloned before primary ownership is transferred so it
        // remains available if backup delegation is selected later.
        let clone = self.primary.clone();
        let mut primary = std::mem::replace(&mut self.primary, clone);
        let backup_request = request.clone();
        let backup = Arc::clone(&self.backup);
        let config = Arc::clone(&self.config);

        Box::pin(async move {
            #[cfg(feature = "tracing")]
            tracing::debug!(fallback = %config.name, "Calling primary service");

            match primary.call(request).await {
                Ok(response) => {
                    let should_fallback = config
                        .handle_response_predicate
                        .as_ref()
                        .is_some_and(|predicate| predicate(&response));
                    if !should_fallback {
                        record_primary_success(&config);
                        return Ok(response);
                    }

                    record_failed_attempt(&config);
                    drop(response);
                    call_backup::<B, Req, Res, E>(backup, backup_request, &config).await
                }
                Err(error) => {
                    let should_fallback = config
                        .handle_predicate
                        .as_ref()
                        .map(|predicate| predicate(&error))
                        .unwrap_or(true);
                    if !should_fallback {
                        record_skipped(&config);
                        return Err(FallbackError::Inner(error));
                    }

                    record_failed_attempt(&config);
                    // Once delegation is selected, the backup readiness/call
                    // result is authoritative, matching the existing closure
                    // service strategy's last-error-wins behavior.
                    drop(error);
                    call_backup::<B, Req, Res, E>(backup, backup_request, &config).await
                }
            }
        })
    }
}

async fn call_backup<B, Req, Res, E>(
    backup: Arc<Mutex<B>>,
    request: Req,
    config: &ServiceFallbackConfig<Res, E>,
) -> Result<Res, FallbackError<E, B::Error>>
where
    B: Service<Req, Response = Res> + Send + 'static,
    B::Future: Send + 'static,
    B::Error: Send + 'static,
    Req: Send + 'static,
{
    #[cfg(feature = "tracing")]
    tracing::debug!(fallback = %config.name, "Waiting for backup service readiness");

    let backup_future = {
        let mut backup = backup.lock().await;
        if let Err(error) = poll_fn(|cx| backup.poll_ready(cx)).await {
            drop(backup);
            record_backup_failure(config);
            return Err(FallbackError::FallbackFailed(error));
        }
        backup.call(request)
    };

    match backup_future.await {
        Ok(response) => {
            record_backup_applied(config);
            Ok(response)
        }
        Err(error) => {
            record_backup_failure(config);
            Err(FallbackError::FallbackFailed(error))
        }
    }
}

fn record_primary_success<Res, E>(config: &ServiceFallbackConfig<Res, E>) {
    #[cfg(feature = "metrics")]
    counter!(
        "fallback_calls_total",
        "fallback" => config.name.clone(),
        "result" => "success"
    )
    .increment(1);

    config.event_listeners.emit(&FallbackEvent::Success {
        pattern_name: config.name.clone(),
        timestamp: Instant::now(),
    });
}

fn record_failed_attempt<Res, E>(config: &ServiceFallbackConfig<Res, E>) {
    config.event_listeners.emit(&FallbackEvent::FailedAttempt {
        pattern_name: config.name.clone(),
        timestamp: Instant::now(),
    });
}

fn record_skipped<Res, E>(config: &ServiceFallbackConfig<Res, E>) {
    #[cfg(feature = "metrics")]
    counter!(
        "fallback_calls_total",
        "fallback" => config.name.clone(),
        "result" => "skipped"
    )
    .increment(1);

    config.event_listeners.emit(&FallbackEvent::Skipped {
        pattern_name: config.name.clone(),
        timestamp: Instant::now(),
    });
}

fn record_backup_applied<Res, E>(config: &ServiceFallbackConfig<Res, E>) {
    #[cfg(feature = "metrics")]
    counter!(
        "fallback_calls_total",
        "fallback" => config.name.clone(),
        "result" => "applied",
        "strategy" => "tower_service"
    )
    .increment(1);

    config.event_listeners.emit(&FallbackEvent::Applied {
        pattern_name: config.name.clone(),
        timestamp: Instant::now(),
        strategy: "tower_service",
    });
}

fn record_backup_failure<Res, E>(config: &ServiceFallbackConfig<Res, E>) {
    #[cfg(feature = "tracing")]
    tracing::warn!(fallback = %config.name, "Backup service failed");

    #[cfg(feature = "metrics")]
    counter!(
        "fallback_calls_total",
        "fallback" => config.name.clone(),
        "result" => "failed",
        "strategy" => "tower_service"
    )
    .increment(1);

    config.event_listeners.emit(&FallbackEvent::Failed {
        pattern_name: config.name.clone(),
        timestamp: Instant::now(),
    });
}
