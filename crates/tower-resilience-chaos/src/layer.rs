//! Tower layer for chaos engineering.

use crate::config::{ChaosConfig, ChaosConfigBuilder};
use crate::service::Chaos;
use tower_layer::Layer;

/// A Tower layer that wraps services with chaos engineering capabilities.
///
/// # Example
///
/// ```rust
/// use tower::ServiceBuilder;
/// use tower_resilience_chaos::ChaosLayer;
/// use std::time::Duration;
///
/// # async fn example() {
/// let chaos = ChaosLayer::<(), std::io::Error>::builder()
///     .name("test-chaos")
///     .error_rate(0.1)  // 10% of requests fail
///     .error_fn(|_req| {
///         std::io::Error::new(std::io::ErrorKind::Other, "chaos!")
///     })
///     .latency_rate(0.2)  // 20% of requests delayed
///     .min_latency(Duration::from_millis(50))
///     .max_latency(Duration::from_millis(200))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(chaos)
///     .service_fn(|req: ()| async { Ok::<_, std::io::Error>(()) });
/// # }
/// ```
#[derive(Clone)]
pub struct ChaosLayer<Req, Err> {
    config: ChaosConfig<Req, Err>,
}

impl<Req, Err> ChaosLayer<Req, Err> {
    /// Create a new builder for chaos layer configuration.
    pub fn builder() -> ChaosConfigBuilder<Req, Err> {
        ChaosConfigBuilder::new()
    }

    /// Create a new chaos layer from configuration.
    pub fn new(config: ChaosConfig<Req, Err>) -> Self {
        Self { config }
    }
}

impl<S, Req, Err> Layer<S> for ChaosLayer<Req, Err>
where
    Req: 'static,
    Err: 'static,
{
    type Service = Chaos<S, Req, Err>;

    fn layer(&self, inner: S) -> Self::Service {
        Chaos::new(inner, self.config.clone())
    }
}
