//! Tower layer for chaos engineering.

use crate::config::{ChaosConfig, ChaosConfigBuilder, NoErrorInjection};
use crate::service::Chaos;
use tower_layer::Layer;

/// A Tower layer that wraps services with chaos engineering capabilities.
///
/// The type parameter `E` is the error injector type:
/// - `ChaosLayer<NoErrorInjection>` - latency-only chaos (works with any types)
/// - `ChaosLayer<CustomErrorFn<F>>` - custom error injection
///
/// # Latency-Only Chaos (no type parameters needed)
///
/// ```rust
/// use tower::ServiceBuilder;
/// use tower_resilience_chaos::ChaosLayer;
/// use std::time::Duration;
///
/// # async fn example() {
/// // No type parameters needed for latency-only chaos!
/// let chaos = ChaosLayer::builder()
///     .latency_rate(0.2)  // 20% of requests delayed
///     .min_latency(Duration::from_millis(50))
///     .max_latency(Duration::from_millis(200))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(chaos)
///     .service_fn(|req: String| async { Ok::<_, std::io::Error>(req) });
/// # }
/// ```
///
/// # Error Injection (types inferred from closure)
///
/// ```rust
/// use tower::ServiceBuilder;
/// use tower_resilience_chaos::ChaosLayer;
///
/// # async fn example() {
/// let chaos = ChaosLayer::builder()
///     .error_rate(0.1)  // 10% of requests fail
///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(chaos)
///     .service_fn(|req: String| async { Ok::<_, std::io::Error>(req) });
/// # }
/// ```
#[derive(Clone)]
pub struct ChaosLayer<E = NoErrorInjection> {
    config: ChaosConfig<E>,
}

impl<E> ChaosLayer<E> {
    /// Create a new chaos layer from configuration.
    pub fn new(config: ChaosConfig<E>) -> Self {
        Self { config }
    }
}

impl ChaosLayer<NoErrorInjection> {
    /// Create a new builder for chaos layer configuration.
    ///
    /// # Example
    ///
    /// ## Latency-only chaos (no type parameters)
    ///
    /// ```rust
    /// use tower_resilience_chaos::ChaosLayer;
    /// use std::time::Duration;
    ///
    /// // No type parameters needed!
    /// let layer = ChaosLayer::builder()
    ///     .latency_rate(0.2)
    ///     .min_latency(Duration::from_millis(50))
    ///     .max_latency(Duration::from_millis(200))
    ///     .build();
    /// ```
    ///
    /// ## Error injection (types inferred from closure)
    ///
    /// ```rust
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .error_rate(0.1)
    ///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
    ///     .build();
    /// ```
    pub fn builder() -> ChaosConfigBuilder<NoErrorInjection> {
        ChaosConfigBuilder::new()
    }
}

// Implement Layer<S> for NoErrorInjection - works with any service
impl<S> Layer<S> for ChaosLayer<NoErrorInjection> {
    type Service = Chaos<S, NoErrorInjection>;

    fn layer(&self, inner: S) -> Self::Service {
        Chaos::new(inner, self.config.clone())
    }
}

// Implement Layer<S> for CustomErrorFn - the closure determines compatible services
impl<S, F> Layer<S> for ChaosLayer<crate::config::CustomErrorFn<F>>
where
    F: 'static,
{
    type Service = Chaos<S, crate::config::CustomErrorFn<F>>;

    fn layer(&self, inner: S) -> Self::Service {
        Chaos::new(inner, self.config.clone())
    }
}
