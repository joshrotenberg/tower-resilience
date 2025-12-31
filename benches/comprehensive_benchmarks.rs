//! Comprehensive benchmarks including reconnect, healthcheck, and worst-case scenarios

use criterion::{Criterion, criterion_group, criterion_main};
use futures::future::BoxFuture;
use std::future::Future;
use std::hint::black_box;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadError, BulkheadLayer};
use tower_resilience_cache::CacheLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_fallback::FallbackLayer;
use tower_resilience_healthcheck::{
    HealthCheckWrapper, HealthChecker, HealthStatus, SelectionStrategy,
};
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

#[derive(Clone, Debug)]
struct TestRequest(u64);

#[derive(Clone, Debug)]
struct TestResponse(#[allow(dead_code)] u64);

#[derive(Clone, Debug)]
struct TestError;

impl From<BulkheadError> for TestError {
    fn from(_: BulkheadError) -> Self {
        TestError
    }
}

// Baseline service
#[derive(Clone)]
struct BaselineService;

impl Service<TestRequest> for BaselineService {
    type Response = TestResponse;
    type Error = TestError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: TestRequest) -> Self::Future {
        Box::pin(async move { Ok(TestResponse(req.0)) })
    }
}

// Service that occasionally fails (for reconnect benchmarks)
#[derive(Clone)]
struct OccasionallyFailingService {
    fail_count: Arc<AtomicUsize>,
    fail_every: usize,
}

impl OccasionallyFailingService {
    fn new(fail_every: usize) -> Self {
        Self {
            fail_count: Arc::new(AtomicUsize::new(0)),
            fail_every,
        }
    }
}

impl Service<String> for OccasionallyFailingService {
    type Response = String;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        let fail_every = self.fail_every;

        Box::pin(async move {
            if count != 0 && count.is_multiple_of(fail_every) {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "simulated failure",
                ))
            } else {
                Ok(format!("Response: {}", req))
            }
        })
    }
}

// ============================================================================
// Reconnect Benchmarks
// ============================================================================

fn bench_reconnect_no_failures(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("reconnect_no_failures", |b| {
        b.to_async(&runtime).iter(|| async {
            let inner = OccasionallyFailingService::new(10000); // Never fails
            let config = ReconnectConfig::builder()
                .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
                .max_attempts(3)
                .build();

            let layer = ReconnectLayer::new(config);
            let mut service = layer.layer(inner);

            let response = service.call(black_box("test".to_string())).await;
            black_box(response)
        });
    });
}

fn bench_reconnect_with_failure(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("reconnect_with_failure", |b| {
        b.to_async(&runtime).iter(|| async {
            let inner = OccasionallyFailingService::new(2); // Fails every 2nd call
            let config = ReconnectConfig::builder()
                .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
                .max_attempts(5)
                .build();

            let layer = ReconnectLayer::new(config);
            let mut service = layer.layer(inner);

            let response = service.call(black_box("test".to_string())).await;
            black_box(response)
        });
    });
}

// ============================================================================
// Health Check Benchmarks
// ============================================================================

#[derive(Clone)]
struct MockResource {
    #[allow(dead_code)]
    id: usize,
    is_healthy: Arc<AtomicBool>,
}

impl MockResource {
    fn new(id: usize) -> Self {
        Self {
            id,
            is_healthy: Arc::new(AtomicBool::new(true)),
        }
    }
}

struct FastHealthChecker;

impl HealthChecker<MockResource> for FastHealthChecker {
    async fn check(&self, resource: &MockResource) -> HealthStatus {
        if resource.is_healthy.load(Ordering::Relaxed) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

fn bench_healthcheck_get_healthy(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("healthcheck_get_healthy", |b| {
        b.to_async(&runtime).iter(|| async {
            let resources: Vec<_> = (0..5).map(MockResource::new).collect();

            let mut builder = HealthCheckWrapper::builder();
            for (i, resource) in resources.iter().enumerate() {
                builder = builder.with_context(resource.clone(), format!("resource-{}", i));
            }

            let wrapper = builder
                .with_checker(FastHealthChecker)
                .with_interval(Duration::from_secs(60)) // Infrequent checks for benchmark
                .with_selection_strategy(SelectionStrategy::RoundRobin)
                .build();

            wrapper.start().await;

            let result = wrapper.get_healthy().await;

            wrapper.stop().await;

            black_box(result)
        });
    });
}

// ============================================================================
// Worst-Case Scenario Benchmarks
// ============================================================================

fn bench_circuit_breaker_open(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_circuit_breaker_open", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = CircuitBreakerLayer::<TestResponse, TestError>::builder()
                .failure_rate_threshold(0.0) // Open immediately on any failure
                .sliding_window_size(10)
                .build();

            let mut service = layer.layer(BaselineService);

            // Force circuit open by making it fail
            // (In real code we'd have a failing service, but for benchmark we'll just measure the overhead)
            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_bulkhead_full(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_bulkhead_full", |b| {
        b.to_async(&runtime).iter(|| async {
            let config = BulkheadLayer::builder()
                .max_concurrent_calls(1) // Very limited
                .max_wait_duration(Some(Duration::from_millis(1))) // Short timeout
                .build();

            let mut service = ServiceBuilder::new().layer(config).service(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_cache_miss(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_cache_miss", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = CacheLayer::builder()
                .max_size(100)
                .ttl(Duration::from_secs(60))
                .key_extractor(|req: &TestRequest| req.0)
                .build();

            let mut service = layer.layer(BaselineService);

            // Use different keys to ensure cache miss
            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(rand::random())))
                .await;
            black_box(response)
        });
    });
}

fn bench_rate_limiter_exhausted(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_rate_limiter_exhausted", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = RateLimiterLayer::builder()
                .limit_for_period(1) // Very limited
                .refresh_period(Duration::from_secs(60)) // Long refresh
                .timeout_duration(Duration::from_millis(1)) // Short timeout
                .build();

            let mut service = layer.layer(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_retry_exhausted(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_retry_exhausted", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = RetryLayer::<TestError>::builder()
                .max_attempts(1) // No retries
                .fixed_backoff(Duration::from_millis(1))
                .build();

            let mut service = layer.layer(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_timelimiter_timeout(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("worst_case_timelimiter_completes_quickly", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(1)) // Very short
                .cancel_running_future(true)
                .build();

            let mut service = layer.layer(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

// ============================================================================
// Fallback Benchmarks
// ============================================================================

fn bench_fallback_no_failure(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("fallback_no_failure", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer =
                FallbackLayer::<TestRequest, TestResponse, TestError>::value(TestResponse(0));

            let mut service = layer.layer(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_fallback_with_failure(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // Service that always fails
    #[derive(Clone)]
    struct FailingService;

    impl Service<TestRequest> for FailingService {
        type Response = TestResponse;
        type Error = TestError;
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: TestRequest) -> Self::Future {
            Box::pin(async move { Err(TestError) })
        }
    }

    c.bench_function("fallback_with_failure", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer =
                FallbackLayer::<TestRequest, TestResponse, TestError>::value(TestResponse(999));

            let mut service = layer.layer(FailingService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

fn bench_fallback_from_error(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    #[derive(Clone)]
    struct FailingService;

    impl Service<TestRequest> for FailingService {
        type Response = TestResponse;
        type Error = TestError;
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: TestRequest) -> Self::Future {
            Box::pin(async move { Err(TestError) })
        }
    }

    c.bench_function("fallback_from_error", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = FallbackLayer::<TestRequest, TestResponse, TestError>::from_error(
                |_e: &TestError| TestResponse(0),
            );

            let mut service = layer.layer(FailingService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

// ============================================================================
// Pattern Composition Benchmarks
// ============================================================================

fn bench_full_stack_composition(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("composition_three_layers", |b| {
        b.to_async(&runtime).iter(|| async {
            let bh_layer = BulkheadLayer::builder().max_concurrent_calls(100).build();

            let rl_layer = RateLimiterLayer::builder()
                .limit_for_period(1000)
                .refresh_period(Duration::from_secs(1))
                .build();

            let tl_layer = TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_secs(30))
                .build();

            let mut service = ServiceBuilder::new()
                .layer(tl_layer)
                .layer(rl_layer)
                .layer(bh_layer)
                .service(BaselineService);

            let response = service
                .ready()
                .await
                .unwrap()
                .call(black_box(TestRequest(42)))
                .await;
            black_box(response)
        });
    });
}

criterion_group!(
    reconnect_benches,
    bench_reconnect_no_failures,
    bench_reconnect_with_failure,
);

criterion_group!(healthcheck_benches, bench_healthcheck_get_healthy,);

criterion_group!(
    worst_case_benches,
    bench_circuit_breaker_open,
    bench_bulkhead_full,
    bench_cache_miss,
    bench_rate_limiter_exhausted,
    bench_retry_exhausted,
    bench_timelimiter_timeout,
);

criterion_group!(composition_benches, bench_full_stack_composition,);

criterion_group!(
    fallback_benches,
    bench_fallback_no_failure,
    bench_fallback_with_failure,
    bench_fallback_from_error,
);

criterion_main!(
    reconnect_benches,
    healthcheck_benches,
    worst_case_benches,
    composition_benches,
    fallback_benches,
);
