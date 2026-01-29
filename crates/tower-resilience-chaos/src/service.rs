//! Chaos service implementation.

use crate::config::{ChaosConfig, ErrorInjector};
use crate::events::ChaosEvent;
use futures::future::BoxFuture;
use rand::rngs::StdRng;
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower_service::Service;

/// A Tower service that injects chaos (errors and latency) into requests.
///
/// The type parameter `E` is the error injector type:
/// - `Chaos<S, NoErrorInjection>` - latency-only chaos
/// - `Chaos<S, CustomErrorFn<F>>` - custom error injection
#[derive(Clone)]
pub struct Chaos<S, E> {
    inner: S,
    config: Arc<ChaosConfig<E>>,
    rng: Arc<Mutex<StdRng>>,
}

impl<S, E> Chaos<S, E> {
    /// Create a new chaos service.
    pub(crate) fn new(inner: S, config: ChaosConfig<E>) -> Self {
        let rng = config.create_rng();
        Self {
            inner,
            config: Arc::new(config),
            rng: Arc::new(Mutex::new(rng)),
        }
    }
}

impl<S, E, Req, Res, Err> Service<Req> for Chaos<S, E>
where
    S: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Req: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
    E: ErrorInjector<Req, Err> + Clone + 'static,
{
    type Response = Res;
    type Error = Err;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let mut inner = self.inner.clone();
        let config = Arc::clone(&self.config);
        let rng = Arc::clone(&self.rng);

        Box::pin(async move {
            let mut should_inject_latency = false;
            let mut latency_duration = Duration::ZERO;
            let mut error_roll: f64 = 1.0; // Default to no error injection

            // Determine what chaos to inject
            {
                let mut rng = rng.lock().unwrap();

                // Check if we should inject an error
                if config.error_injector.error_rate() > 0.0 {
                    error_roll = rng.random();
                }

                // Check if we should inject latency (only if not injecting error)
                if config.latency_rate > 0.0 && error_roll >= config.error_injector.error_rate() {
                    let latency_roll: f64 = rng.random();
                    should_inject_latency = latency_roll < config.latency_rate;

                    if should_inject_latency {
                        let min_ms = config.min_latency.as_millis() as u64;
                        let max_ms = config.max_latency.as_millis() as u64;
                        let delay_ms = if max_ms > min_ms {
                            rng.random_range(min_ms..=max_ms)
                        } else {
                            min_ms
                        };
                        latency_duration = Duration::from_millis(delay_ms);
                    }
                }
            }

            // Check if error injection should happen
            if let Some(err) = config.error_injector.inject_error(&req, error_roll) {
                let event = ChaosEvent::ErrorInjected {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                };
                config.event_listeners.emit(&event);

                #[cfg(feature = "tracing")]
                tracing::warn!(
                    chaos_layer = %config.name,
                    "chaos: error injected"
                );

                #[cfg(feature = "metrics")]
                metrics::counter!("chaos.errors_injected", "layer" => config.name.clone())
                    .increment(1);

                return Err(err);
            }

            // Inject latency if determined
            if should_inject_latency {
                let event = ChaosEvent::LatencyInjected {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                    delay: latency_duration,
                };
                config.event_listeners.emit(&event);

                #[cfg(feature = "tracing")]
                tracing::debug!(
                    chaos_layer = %config.name,
                    delay_ms = latency_duration.as_millis(),
                    "chaos: latency injected"
                );

                #[cfg(feature = "metrics")]
                {
                    metrics::counter!("chaos.latency_injections", "layer" => config.name.clone())
                        .increment(1);
                    metrics::histogram!("chaos.injected_latency_ms", "layer" => config.name.clone())
                        .record(latency_duration.as_millis() as f64);
                }

                tokio::time::sleep(latency_duration).await;
            }

            // Pass through (no chaos or after latency)
            if !should_inject_latency {
                let event = ChaosEvent::PassedThrough {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                };
                config.event_listeners.emit(&event);

                #[cfg(feature = "metrics")]
                metrics::counter!("chaos.passed_through", "layer" => config.name.clone())
                    .increment(1);
            }

            inner.call(req).await
        })
    }
}
