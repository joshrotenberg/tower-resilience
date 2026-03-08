//! Weighted traffic routing for Tower services.
//!
//! This crate provides a `WeightedRouter` service that distributes requests
//! across multiple backend services based on configured weights. It is designed
//! for canary deployments, progressive rollouts, and controlled traffic splitting.
//!
//! # Overview
//!
//! Unlike other tower-resilience patterns which wrap a single service and modify
//! its behavior, `WeightedRouter` *selects among* multiple services. It is a
//! standalone `Service`, not a `Layer`.
//!
//! All backend services must have the same `Request`, `Response`, and `Error`
//! types. For canary deployments (same service, different version), this is
//! the natural case.
//!
//! # Selection Strategies
//!
//! - **Deterministic** (default): Uses an atomic counter for predictable,
//!   repeatable distribution. With weights `[90, 10]`, every cycle of 100
//!   requests sends exactly 90 to the first backend and 10 to the second.
//!
//! - **Random**: Each request independently selects a backend with probability
//!   proportional to its weight. Better for high-volume statistical distribution,
//!   but may show variance at low traffic.
//!
//! # Readiness
//!
//! `poll_ready` checks that **all** backends are ready. This is the simplest
//! and most predictable contract. If a backend is slow or failing, pair it
//! with a circuit breaker so that readiness resolves quickly (open circuit =
//! immediate ready or error).
//!
//! # Example
//!
//! Because all backends must be the same type `S`, use `BoxService` when
//! constructing from different closures:
//!
//! ```rust,no_run
//! use tower::Service;
//! use tower::util::BoxService;
//! use tower_resilience_router::WeightedRouter;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let svc_v1: BoxService<String, String, std::io::Error> =
//!     BoxService::new(tower::service_fn(|req: String| async move {
//!         Ok(format!("v1: {}", req))
//!     }));
//! let svc_v2: BoxService<String, String, std::io::Error> =
//!     BoxService::new(tower::service_fn(|req: String| async move {
//!         Ok(format!("v2: {}", req))
//!     }));
//!
//! let mut router = WeightedRouter::builder()
//!     .route(svc_v1, 90)
//!     .route(svc_v2, 10)
//!     .build();
//! # Ok(())
//! # }
//! ```
//!
//! # Composability
//!
//! The natural composition pattern puts resilience middleware *inside* each backend:
//!
//! ```rust,no_run
//! use tower::Layer;
//! use tower_resilience_router::WeightedRouter;
//! # use tower::service_fn;
//! # let svc_v1 = service_fn(|_: ()| async { Ok::<_, std::io::Error>(()) });
//! # let svc_v2 = service_fn(|_: ()| async { Ok::<_, std::io::Error>(()) });
//!
//! // Each backend gets its own circuit breaker
//! // let cb = CircuitBreakerLayer::standard().build();
//! // let router = WeightedRouter::builder()
//! //     .route(cb.layer(svc_v1), 90)
//! //     .route(cb.layer(svc_v2), 10)
//! //     .build();
//! ```

pub mod config;
pub mod error;
pub mod events;
pub mod selection;

pub use config::WeightedRouterBuilder;
pub use error::WeightedRouterError;
pub use events::RouterEvent;
pub use selection::SelectionStrategy;

use config::RouterConfig;
use selection::WeightedSelector;
use std::task::{Context, Poll};
use tower_service::Service;

/// A service that routes requests to one of several backends based on weights.
///
/// `WeightedRouter` is a standalone `Service`, not a `Layer`. It selects among
/// multiple backend services of the same type, distributing traffic according
/// to configured weights.
///
/// Use [`WeightedRouter::builder`] to construct a new router.
pub struct WeightedRouter<S> {
    /// Backend services with their weights.
    backends: Vec<(S, u32)>,
    /// Selector for choosing backends.
    selector: WeightedSelector,
    /// Configuration.
    config: RouterConfig,
}

impl<S> WeightedRouter<S> {
    /// Creates a new builder for configuring a `WeightedRouter`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use tower_resilience_router::WeightedRouter;
    /// use tower::util::BoxService;
    ///
    /// let svc_v1: BoxService<(), (), std::io::Error> =
    ///     BoxService::new(tower::service_fn(|_: ()| async { Ok(()) }));
    /// let svc_v2: BoxService<(), (), std::io::Error> =
    ///     BoxService::new(tower::service_fn(|_: ()| async { Ok(()) }));
    ///
    /// let router = WeightedRouter::builder()
    ///     .route(svc_v1, 90)
    ///     .route(svc_v2, 10)
    ///     .build();
    /// ```
    pub fn builder() -> WeightedRouterBuilder<S> {
        WeightedRouterBuilder::new()
    }

    pub(crate) fn new(backends: Vec<(S, u32)>, config: RouterConfig) -> Self {
        let weights: Vec<u32> = backends.iter().map(|(_, w)| *w).collect();
        let selector = WeightedSelector::new(&weights, config.strategy);
        Self {
            backends,
            selector,
            config,
        }
    }

    /// Returns the number of backends.
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    /// Returns the weights of all backends.
    pub fn weights(&self) -> Vec<u32> {
        self.backends.iter().map(|(_, w)| *w).collect()
    }

    /// Returns the name of this router instance.
    pub fn name(&self) -> &str {
        &self.config.name
    }
}

impl<S: Clone> Clone for WeightedRouter<S> {
    fn clone(&self) -> Self {
        Self {
            backends: self.backends.clone(),
            selector: self.selector.clone(),
            config: self.config.clone(),
        }
    }
}

impl<S, Request> Service<Request> for WeightedRouter<S>
where
    S: Service<Request>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // All backends must be ready.
        for (svc, _) in &mut self.backends {
            match svc.poll_ready(cx)? {
                Poll::Ready(()) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let idx = self.selector.select();
        let (svc, weight) = &mut self.backends[idx];

        #[cfg(feature = "metrics")]
        {
            let labels = [
                ("router", self.config.name.clone()),
                ("backend", idx.to_string()),
            ];
            metrics::counter!("router_requests_routed_total", &labels).increment(1);
        }

        #[cfg(feature = "tracing")]
        {
            tracing::debug!(
                router = %self.config.name,
                backend_index = idx,
                backend_weight = *weight,
                "routing request to backend"
            );
        }

        self.config
            .event_listeners
            .emit(&RouterEvent::RequestRouted {
                pattern_name: self.config.name.clone(),
                timestamp: std::time::Instant::now(),
                backend_index: idx,
                backend_weight: *weight,
            });

        svc.call(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::util::BoxService;
    use tower::ServiceExt;

    type BoxSvc = BoxService<(), &'static str, TestError>;

    #[derive(Clone, Debug)]
    struct TestError;
    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "test error")
        }
    }
    impl std::error::Error for TestError {}

    fn counting_svc(counter: Arc<AtomicUsize>, label: &'static str) -> BoxSvc {
        BoxService::new(tower::service_fn(move |_: ()| {
            let c = Arc::clone(&counter);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(label)
            }
        }))
    }

    #[tokio::test]
    async fn routes_by_weight_deterministic() {
        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));

        let mut router = WeightedRouter::builder()
            .route(counting_svc(Arc::clone(&count_a), "a"), 80)
            .route(counting_svc(Arc::clone(&count_b), "b"), 20)
            .build();

        for _ in 0..100 {
            let _ = router.ready().await.unwrap().call(()).await;
        }

        assert_eq!(count_a.load(Ordering::SeqCst), 80);
        assert_eq!(count_b.load(Ordering::SeqCst), 20);
    }

    #[tokio::test]
    async fn single_backend_gets_all_traffic() {
        let count = Arc::new(AtomicUsize::new(0));

        let mut router = WeightedRouter::builder()
            .route(counting_svc(Arc::clone(&count), "ok"), 1)
            .build();

        for _ in 0..50 {
            let _ = router.ready().await.unwrap().call(()).await;
        }

        assert_eq!(count.load(Ordering::SeqCst), 50);
    }

    #[tokio::test]
    async fn three_backends() {
        let counts: Vec<Arc<AtomicUsize>> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();

        let mut router = WeightedRouter::builder()
            .route(counting_svc(Arc::clone(&counts[0]), "0"), 50)
            .route(counting_svc(Arc::clone(&counts[1]), "1"), 30)
            .route(counting_svc(Arc::clone(&counts[2]), "2"), 20)
            .build();

        for _ in 0..100 {
            let _ = router.ready().await.unwrap().call(()).await;
        }

        assert_eq!(counts[0].load(Ordering::SeqCst), 50);
        assert_eq!(counts[1].load(Ordering::SeqCst), 30);
        assert_eq!(counts[2].load(Ordering::SeqCst), 20);
    }

    #[tokio::test]
    async fn error_propagates_from_backend() {
        let svc: BoxSvc = BoxService::new(tower::service_fn(|_: ()| async {
            Err::<&str, _>(TestError)
        }));

        let mut router = WeightedRouter::builder().route(svc, 1).build();

        let result = router.ready().await.unwrap().call(()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn event_listener_fires() {
        let routed_count = Arc::new(AtomicUsize::new(0));
        let rc = Arc::clone(&routed_count);

        let svc: BoxSvc = BoxService::new(tower::service_fn(|_: ()| async {
            Ok::<_, TestError>("ok")
        }));

        let mut router = WeightedRouter::builder()
            .route(svc, 1)
            .on_request_routed(move |_idx, _weight| {
                rc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        for _ in 0..5 {
            let _ = router.ready().await.unwrap().call(()).await;
        }

        assert_eq!(routed_count.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn builder_accessors() {
        let router = WeightedRouter::builder()
            .name("canary")
            .route(counting_svc(Arc::new(AtomicUsize::new(0)), "a"), 90)
            .route(counting_svc(Arc::new(AtomicUsize::new(0)), "b"), 10)
            .build();

        assert_eq!(router.backend_count(), 2);
        assert_eq!(router.weights(), vec![90, 10]);
        assert_eq!(router.name(), "canary");
    }

    #[test]
    #[should_panic(expected = "at least one backend is required")]
    fn panics_on_no_backends() {
        let _router: WeightedRouter<BoxSvc> = WeightedRouter::builder().build();
    }

    #[test]
    #[should_panic(expected = "weight 0")]
    fn panics_on_zero_weight() {
        let svc: BoxSvc = BoxService::new(tower::service_fn(|_: ()| async {
            Ok::<_, TestError>("ok")
        }));
        let _router = WeightedRouter::builder().route(svc, 0).build();
    }

    #[tokio::test]
    async fn random_strategy_converges() {
        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));

        let mut router = WeightedRouter::builder()
            .route(counting_svc(Arc::clone(&count_a), "a"), 80)
            .route(counting_svc(Arc::clone(&count_b), "b"), 20)
            .random()
            .build();

        let total = 10_000;
        for _ in 0..total {
            let _ = router.ready().await.unwrap().call(()).await;
        }

        let a = count_a.load(Ordering::SeqCst);
        let ratio = a as f64 / total as f64;
        assert!(
            (0.75..=0.85).contains(&ratio),
            "expected ~80%, got {:.1}%",
            ratio * 100.0
        );
    }
}
