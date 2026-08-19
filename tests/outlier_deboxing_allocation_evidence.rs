//! Allocation-count evidence for #426.
//!
//! `tower-resilience-outlier` was de-boxed from `Service::Future =
//! BoxFuture<'static, Result<...>>` to a hand-written, `pin_project!`-based
//! `OutlierDetectionFuture<F, C>` enum (see `crates/tower-resilience-outlier/
//! src/service.rs`). This binary is a standalone integration test (its own
//! process, so installing a custom `#[global_allocator]` here cannot affect
//! any other test binary) that counts heap allocations for one `call()` on
//! the de-boxed implementation versus a byte-for-byte reconstruction of the
//! pre-#426 `BoxFuture`-based implementation, to give a real number for the
//! tradeoff documented in `docs/tower-api-surface-audit.md`.
//!
//! Both implementations do the identical amount of "real" work per call
//! (clone the instance name, clone the shared detector handle, drive the
//! inner future, classify and record the result), so the measured
//! allocation delta isolates the cost of the `Box::pin` heap allocation
//! itself.

use std::alloc::{GlobalAlloc, Layout, System};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use tower::{Layer, Service, ServiceExt};
use tower_resilience_outlier::{
    DefaultClassifier, FailureClassifier, OutlierDetectionLayer, OutlierDetectionServiceError,
    OutlierDetector,
};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// A reconstruction of `OutlierDetectionService`'s pre-#426 `Service` impl:
/// the same clone/classify/record logic, but with `Self::Future` boxed via
/// `Box::pin` the way every one of #426's other nine candidate crates still
/// does. Kept local to this test since the real crate no longer contains it.
#[derive(Clone)]
struct BoxedOutlierLike<S, C> {
    inner: S,
    detector: OutlierDetector,
    instance_name: String,
    classifier: C,
}

impl<S, C, Request> Service<Request> for BoxedOutlierLike<S, C>
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
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(OutlierDetectionServiceError::Inner)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let instance_name = self.instance_name.clone();
        let detector = self.detector.clone();
        let classifier = self.classifier.clone();

        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            let result = inner.call(request).await;

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

#[tokio::test]
async fn deboxing_reduces_per_call_heap_allocations() {
    // De-boxed (current) implementation.
    let detector = OutlierDetector::new();
    detector.register("backend", 5);
    let mut deboxed = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("backend")
        .build()
        .unwrap()
        .layer(tower::service_fn(|req: u32| async move {
            Ok::<u32, std::io::Error>(req)
        }));

    // Warm up so any one-time allocator/executor setup doesn't pollute the
    // measured count.
    let _ = deboxed.ready().await.unwrap().call(0).await;

    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let _ = deboxed.ready().await.unwrap().call(1).await;
    let deboxed_allocs = ALLOC_COUNT.load(Ordering::Relaxed) - before;

    // Pre-#426 BoxFuture-based equivalent.
    let boxed_detector = OutlierDetector::new();
    boxed_detector.register("backend", 5);
    let mut boxed = BoxedOutlierLike {
        inner: tower::service_fn(|req: u32| async move { Ok::<u32, std::io::Error>(req) }),
        detector: boxed_detector,
        instance_name: "backend".to_string(),
        classifier: DefaultClassifier,
    };

    let _ = boxed.ready().await.unwrap().call(0).await;

    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let _ = boxed.ready().await.unwrap().call(1).await;
    let boxed_allocs = ALLOC_COUNT.load(Ordering::Relaxed) - before;

    println!("de-boxed per-call allocations:        {deboxed_allocs}");
    println!("boxed-equivalent per-call allocations: {boxed_allocs}");

    assert!(
        deboxed_allocs < boxed_allocs,
        "expected the de-boxed implementation to allocate fewer times per \
         call than the BoxFuture-based equivalent; de-boxed={deboxed_allocs}, \
         boxed={boxed_allocs}"
    );
}
