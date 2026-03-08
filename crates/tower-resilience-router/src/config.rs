//! Configuration for the weighted router.

use crate::events::RouterEvent;
use crate::selection::SelectionStrategy;
use tower_resilience_core::events::{EventListeners, FnListener};

/// Configuration for the weighted router.
#[derive(Clone)]
pub struct RouterConfig {
    /// Name of this router instance.
    pub(crate) name: String,
    /// Selection strategy for choosing backends.
    pub(crate) strategy: SelectionStrategy,
    /// Event listeners.
    pub(crate) event_listeners: EventListeners<RouterEvent>,
}

/// Builder for configuring a `WeightedRouter`.
///
/// Use [`WeightedRouter::builder`](crate::WeightedRouter::builder) to create a new builder.
pub struct WeightedRouterBuilder<S> {
    backends: Vec<(S, u32)>,
    name: String,
    strategy: SelectionStrategy,
    event_listeners: EventListeners<RouterEvent>,
}

impl<S> WeightedRouterBuilder<S> {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            name: "weighted_router".to_string(),
            strategy: SelectionStrategy::default(),
            event_listeners: EventListeners::new(),
        }
    }

    /// Adds a backend service with the given weight.
    ///
    /// Higher weights receive proportionally more traffic. A backend with
    /// weight 90 receives 9x the traffic of a backend with weight 10.
    ///
    /// # Panics
    ///
    /// The [`build`](Self::build) method panics if no backends are added
    /// or if any weight is zero.
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
    pub fn route(mut self, service: S, weight: u32) -> Self {
        self.backends.push((service, weight));
        self
    }

    /// Sets the name of this router instance.
    ///
    /// Used for metrics labels and event identification.
    ///
    /// Default: `"weighted_router"`
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the selection strategy.
    ///
    /// Default: [`SelectionStrategy::Deterministic`]
    pub fn strategy(mut self, strategy: SelectionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Uses deterministic selection (default).
    ///
    /// Requests are distributed using an atomic counter. With weights
    /// `[90, 10]`, every 100th request cycle distributes exactly 90
    /// requests to the first backend and 10 to the second.
    pub fn deterministic(mut self) -> Self {
        self.strategy = SelectionStrategy::Deterministic;
        self
    }

    /// Uses random selection.
    ///
    /// Each request independently selects a backend based on weighted
    /// random probability. Over many requests, the distribution converges
    /// to the configured weights, but individual request sequences may
    /// vary -- especially at low traffic volumes.
    pub fn random(mut self) -> Self {
        self.strategy = SelectionStrategy::Random;
        self
    }

    /// Registers a callback when a request is routed to a backend.
    ///
    /// # Callback Signature
    /// `Fn(usize, u32)` - Called with the backend index and its weight.
    pub fn on_request_routed<F>(mut self, f: F) -> Self
    where
        F: Fn(usize, u32) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            let RouterEvent::RequestRouted {
                backend_index,
                backend_weight,
                ..
            } = event;
            f(*backend_index, *backend_weight);
        }));
        self
    }

    /// Builds the `WeightedRouter`.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - No backends have been added
    /// - Any backend has a weight of zero
    pub fn build(self) -> crate::WeightedRouter<S> {
        assert!(
            !self.backends.is_empty(),
            "at least one backend is required"
        );
        for (i, (_, weight)) in self.backends.iter().enumerate() {
            assert!(
                *weight > 0,
                "backend {i} has weight 0; all weights must be positive"
            );
        }

        let config = RouterConfig {
            name: self.name,
            strategy: self.strategy,
            event_listeners: self.event_listeners,
        };

        crate::WeightedRouter::new(self.backends, config)
    }
}

impl<S> Default for WeightedRouterBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}
