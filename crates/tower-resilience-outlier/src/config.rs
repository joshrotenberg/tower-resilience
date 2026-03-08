//! Configuration and builder for the outlier detection middleware.

use crate::detector::OutlierDetector;
use crate::layer::OutlierDetectionLayer;
use tower_resilience_core::classifier::{DefaultClassifier, FnClassifier};

/// Configuration for an outlier detection instance.
#[derive(Clone)]
pub struct OutlierDetectionConfig<C = DefaultClassifier> {
    /// The shared fleet-level detector.
    pub(crate) detector: OutlierDetector,
    /// The name of this instance within the fleet.
    pub(crate) instance_name: String,
    /// The failure classifier.
    pub(crate) classifier: C,
    /// Whether to use backpressure mode (poll_ready returns Pending).
    pub(crate) backpressure: bool,
}

/// Builder for configuring outlier detection instances.
pub struct OutlierDetectionConfigBuilder<C = DefaultClassifier> {
    detector: Option<OutlierDetector>,
    instance_name: String,
    classifier: C,
    backpressure: bool,
}

impl OutlierDetectionConfigBuilder<DefaultClassifier> {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self {
            detector: None,
            instance_name: "instance".to_string(),
            classifier: DefaultClassifier,
            backpressure: true, // Backpressure is the default
        }
    }

    /// Sets a custom failure classifier function.
    ///
    /// This replaces the default classifier (which treats all `Err` results as failures)
    /// with a custom function that can inspect both `Ok` and `Err` results.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_outlier::OutlierDetectionLayer;
    /// use tower_resilience_outlier::OutlierDetector;
    ///
    /// let detector = OutlierDetector::new();
    /// detector.register("backend-1", 5);
    ///
    /// let layer = OutlierDetectionLayer::builder()
    ///     .detector(detector)
    ///     .instance_name("backend-1")
    ///     .failure_classifier(|result: &Result<String, std::io::Error>| {
    ///         match result {
    ///             Ok(_) => false,
    ///             Err(e) => e.kind() == std::io::ErrorKind::ConnectionRefused,
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn failure_classifier<F>(self, f: F) -> OutlierDetectionConfigBuilder<FnClassifier<F>> {
        OutlierDetectionConfigBuilder {
            detector: self.detector,
            instance_name: self.instance_name,
            classifier: FnClassifier::new(f),
            backpressure: self.backpressure,
        }
    }
}

impl<C> OutlierDetectionConfigBuilder<C> {
    /// Sets the shared fleet-level detector.
    ///
    /// The detector coordinates ejection state across all instances
    /// and enforces the `max_ejection_percent` limit.
    pub fn detector(mut self, detector: OutlierDetector) -> Self {
        self.detector = Some(detector);
        self
    }

    /// Sets the name of this instance within the fleet.
    ///
    /// This name is used to identify the instance in the shared detector
    /// and in events.
    pub fn instance_name(mut self, name: impl Into<String>) -> Self {
        self.instance_name = name.into();
        self
    }

    /// Enables error-on-ejection mode instead of the default backpressure mode.
    ///
    /// In error mode, `call()` returns `OutlierDetectionError::Ejected` when
    /// the instance is ejected. In backpressure mode (default), `poll_ready()`
    /// returns `Pending`, which integrates naturally with Tower load balancers.
    pub fn error_on_ejection(mut self) -> Self {
        self.backpressure = false;
        self
    }

    /// Enables backpressure mode (the default).
    ///
    /// In backpressure mode, `poll_ready()` returns `Pending` when the
    /// instance is ejected, allowing Tower load balancers to route around it.
    pub fn backpressure(mut self) -> Self {
        self.backpressure = true;
        self
    }

    /// Builds the `OutlierDetectionLayer`.
    ///
    /// # Panics
    ///
    /// Panics if no detector was provided.
    pub fn build(self) -> OutlierDetectionLayer<C> {
        let detector = self
            .detector
            .expect("OutlierDetectionConfigBuilder requires a detector");
        let config = OutlierDetectionConfig {
            detector,
            instance_name: self.instance_name,
            classifier: self.classifier,
            backpressure: self.backpressure,
        };
        OutlierDetectionLayer::new(config)
    }
}

impl Default for OutlierDetectionConfigBuilder<DefaultClassifier> {
    fn default() -> Self {
        Self::new()
    }
}
