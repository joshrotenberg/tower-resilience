//! Tower Service implementation for outlier detection.

use crate::config::OutlierDetectionConfig;
use crate::error::{OutlierDetectionError, OutlierDetectionServiceError};
use futures::future::BoxFuture;
use std::task::{Context, Poll};
use tower::Service;
use tower_resilience_core::classifier::FailureClassifier;

/// A Tower Service that applies outlier detection to an inner service.
///
/// This wraps a single backend instance. The shared
/// [`OutlierDetector`](crate::OutlierDetector) coordinates ejection state
/// across all instances.
///
/// # Backpressure vs Error Mode
///
/// - **Backpressure (default)**: `poll_ready()` returns `Pending` when ejected,
///   causing Tower load balancers to route around this instance.
/// - **Error mode**: `call()` returns `OutlierDetectionError::Ejected`.
pub struct OutlierDetectionService<S, C> {
    inner: S,
    config: OutlierDetectionConfig<C>,
}

impl<S, C> OutlierDetectionService<S, C> {
    /// Creates a new `OutlierDetectionService`.
    pub(crate) fn new(inner: S, config: OutlierDetectionConfig<C>) -> Self {
        Self { inner, config }
    }

    /// Returns the name of this instance.
    pub fn instance_name(&self) -> &str {
        &self.config.instance_name
    }

    /// Returns `true` if this instance is currently ejected.
    pub fn is_ejected(&self) -> bool {
        self.config.detector.is_ejected(&self.config.instance_name)
    }
}

impl<S: Clone, C: Clone> Clone for OutlierDetectionService<S, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: self.config.clone(),
        }
    }
}

impl<S, C, Request> Service<Request> for OutlierDetectionService<S, C>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    C: FailureClassifier<S::Response, S::Error> + Clone + Send + 'static,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = OutlierDetectionServiceError<S::Error>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // In backpressure mode, return Pending when ejected
        if self.config.backpressure && self.config.detector.is_ejected(&self.config.instance_name) {
            // We need to register a waker. Since recovery is timer-based,
            // we'll wake on the next poll cycle. The load balancer will
            // try other backends in the meantime.
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        // Check inner service readiness
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(OutlierDetectionServiceError::Inner(e))),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let instance_name = self.config.instance_name.clone();
        let detector = self.config.detector.clone();
        let classifier = self.config.classifier.clone();
        let backpressure = self.config.backpressure;

        // In error mode, check ejection in call()
        if !backpressure && detector.is_ejected(&instance_name) {
            return Box::pin(async move {
                Err(OutlierDetectionServiceError::OutlierDetection(
                    OutlierDetectionError::Ejected {
                        name: instance_name,
                    },
                ))
            });
        }

        let mut inner = self.inner.clone();
        // Swap so the clone becomes the "pending" one
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move {
            let result = inner.call(request).await;

            // Classify the result
            let is_failure = classifier.classify(&result);

            if is_failure {
                detector.record_failure(&instance_name);
            } else {
                detector.record_success(&instance_name);
            }

            result.map_err(OutlierDetectionServiceError::Inner)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutlierDetector;
    use tower::ServiceExt;

    async fn make_service(
        detector: OutlierDetector,
        name: &str,
        backpressure: bool,
    ) -> OutlierDetectionService<
        tower::util::BoxCloneService<String, String, std::io::Error>,
        tower_resilience_core::classifier::DefaultClassifier,
    > {
        let svc = tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) });
        let boxed = tower::util::BoxCloneService::new(svc);
        let mut builder = crate::OutlierDetectionLayer::builder()
            .detector(detector)
            .instance_name(name);
        if !backpressure {
            builder = builder.error_on_ejection();
        }
        let layer = builder.build();
        tower::Layer::layer(&layer, boxed)
    }

    #[tokio::test]
    async fn test_healthy_instance_passes_through() {
        let detector = OutlierDetector::new();
        detector.register("backend-1", 5);

        let mut svc = make_service(detector, "backend-1", false).await;
        let resp = svc.ready().await.unwrap().call("hello".to_string()).await;
        assert!(resp.is_ok());
        assert_eq!(resp.unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_error_mode_rejects_ejected() {
        let detector = OutlierDetector::new().max_ejection_percent(100);
        detector.register("backend-1", 1);

        // Create a failing service to trigger ejection
        let fail_svc = tower::service_fn(|_req: String| async {
            Err::<String, std::io::Error>(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "down",
            ))
        });
        let boxed = tower::util::BoxCloneService::new(fail_svc);
        let layer = crate::OutlierDetectionLayer::builder()
            .detector(detector.clone())
            .instance_name("backend-1")
            .error_on_ejection()
            .build();
        let mut svc = tower::Layer::layer(&layer, boxed);

        // First call fails (inner error) and triggers ejection
        let resp = svc.ready().await.unwrap().call("hello".to_string()).await;
        assert!(resp.is_err());
        assert!(resp.unwrap_err().is_inner());

        // Second call should be rejected by outlier detection
        let resp = svc.ready().await.unwrap().call("hello".to_string()).await;
        assert!(resp.is_err());
        assert!(resp.unwrap_err().is_outlier_detection());
    }
}
