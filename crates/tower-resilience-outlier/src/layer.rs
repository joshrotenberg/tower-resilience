//! Tower Layer implementation for outlier detection.

use crate::config::{OutlierDetectionConfig, OutlierDetectionConfigBuilder};
use crate::service::OutlierDetectionService;
use tower::Layer;
use tower_resilience_core::classifier::DefaultClassifier;

/// A Tower Layer that applies outlier detection behavior to an inner service.
///
/// Each `OutlierDetectionLayer` wraps a single backend instance. Multiple layers
/// sharing the same [`OutlierDetector`](crate::OutlierDetector) coordinate to
/// provide fleet-aware ejection.
///
/// # Examples
///
/// ```rust
/// use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};
/// use tower::{ServiceBuilder, service_fn};
///
/// let detector = OutlierDetector::new()
///     .max_ejection_percent(50);
///
/// detector.register("backend-1", 5);
///
/// let layer = OutlierDetectionLayer::builder()
///     .detector(detector)
///     .instance_name("backend-1")
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) }));
/// ```
#[derive(Clone)]
pub struct OutlierDetectionLayer<C = DefaultClassifier> {
    config: OutlierDetectionConfig<C>,
}

impl<C> OutlierDetectionLayer<C> {
    /// Creates a new `OutlierDetectionLayer` from the given configuration.
    pub(crate) fn new(config: OutlierDetectionConfig<C>) -> Self {
        Self { config }
    }
}

impl OutlierDetectionLayer<DefaultClassifier> {
    /// Creates a new builder for configuring an outlier detection layer.
    pub fn builder() -> OutlierDetectionConfigBuilder<DefaultClassifier> {
        OutlierDetectionConfigBuilder::new()
    }
}

// Implement Layer<S> for any classifier type `C`. The `C: Clone` bound is only
// needed to clone the config into the service; which services the resulting
// `OutlierDetectionService` can serve is decided by the classifier's
// `FailureClassifier<Res, Err>` impl at the wrap point. This covers
// `DefaultClassifier`, `FnClassifier<F>`, and any named classifier passed to
// `failure_classifier_type`.
impl<S, C> Layer<S> for OutlierDetectionLayer<C>
where
    C: Clone,
{
    type Service = OutlierDetectionService<S, C>;

    fn layer(&self, service: S) -> Self::Service {
        OutlierDetectionService::new(service, self.config.clone())
    }
}
