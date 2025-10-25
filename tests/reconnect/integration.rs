use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

/// Simulated service that fails for the first N calls
#[derive(Clone)]
struct FailingService {
    fail_count: Arc<AtomicUsize>,
    max_fails: usize,
}

impl FailingService {
    fn new(max_fails: usize) -> Self {
        Self {
            fail_count: Arc::new(AtomicUsize::new(0)),
            max_fails,
        }
    }
}

impl Service<String> for FailingService {
    type Response = String;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        let max_fails = self.max_fails;

        Box::pin(async move {
            if count < max_fails {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "mock connection failure",
                ))
            } else {
                Ok(format!("Response: {}", req))
            }
        })
    }
}

#[tokio::test]
async fn reconnect_succeeds_after_retries() {
    let inner = FailingService::new(2); // Fail first 2 attempts
    let attempts_tracker = inner.fail_count.clone();

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(10)))
        .max_attempts(5)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let result = service.call("test".to_string()).await;

    assert!(result.is_ok());
    assert_eq!(
        attempts_tracker.load(Ordering::SeqCst),
        3,
        "Should take 3 attempts (2 failures + 1 success)"
    );
}

#[tokio::test]
async fn reconnect_respects_max_attempts() {
    let inner = FailingService::new(10); // Never succeeds within limit

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(3)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let result = service.call("test".to_string()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn exponential_backoff_policy() {
    let inner = FailingService::new(3);
    let attempts_tracker = inner.fail_count.clone();

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(10),
            Duration::from_millis(100),
        ))
        .max_attempts(5)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let start = std::time::Instant::now();
    let result = service.call("test".to_string()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert_eq!(attempts_tracker.load(Ordering::SeqCst), 4);

    // Should have some delay due to exponential backoff
    // First retry: ~10ms, second: ~20ms, third: ~40ms = ~70ms minimum
    assert!(
        elapsed.as_millis() >= 30,
        "Expected at least 30ms with backoff, got {:?}",
        elapsed
    );
}

#[tokio::test]
async fn unlimited_attempts() {
    let inner = FailingService::new(10);
    let attempts_tracker = inner.fail_count.clone();

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .unlimited_attempts()
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let result = service.call("test".to_string()).await;

    assert!(result.is_ok());
    assert!(
        attempts_tracker.load(Ordering::SeqCst) >= 11,
        "Should keep trying until success"
    );
}

#[tokio::test]
async fn no_reconnect_policy_fails_immediately() {
    let inner = FailingService::new(1);
    let attempts_tracker = inner.fail_count.clone();

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::None)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let result = service.call("test".to_string()).await;

    assert!(result.is_err());
    assert_eq!(
        attempts_tracker.load(Ordering::SeqCst),
        1,
        "Should only try once with None policy"
    );
}

#[tokio::test]
async fn successful_on_first_attempt() {
    let inner = FailingService::new(0); // Never fails
    let attempts_tracker = inner.fail_count.clone();

    let config = ReconnectConfig::default();
    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let result = service.call("test".to_string()).await;

    assert!(result.is_ok());
    assert_eq!(
        attempts_tracker.load(Ordering::SeqCst),
        1,
        "Should succeed on first attempt"
    );
}
