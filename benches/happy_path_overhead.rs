use criterion::{Criterion, criterion_group, criterion_main};
use futures::future::BoxFuture;
use std::hint::black_box;
use std::time::Duration;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadError, BulkheadLayer};
use tower_resilience_cache::CacheLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_ratelimiter::RateLimiterLayer;
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

// Baseline service that just passes through
#[derive(Clone)]
struct BaselineService;

impl Service<TestRequest> for BaselineService {
    type Response = TestResponse;
    type Error = TestError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: TestRequest) -> Self::Future {
        Box::pin(async move { Ok(TestResponse(req.0)) })
    }
}

fn bench_baseline(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("baseline_no_middleware", |b| {
        b.to_async(&runtime).iter(|| async {
            let mut service = BaselineService;
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

fn bench_circuit_breaker(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("circuitbreaker_closed", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = CircuitBreakerLayer::<TestResponse, TestError>::builder()
                .failure_rate_threshold(0.5)
                .sliding_window_size(100)
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

fn bench_bulkhead(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("bulkhead_permits_available", |b| {
        b.to_async(&runtime).iter(|| async {
            let config = BulkheadLayer::builder().max_concurrent_calls(100).build();
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

fn bench_retry(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("retry_no_retries_needed", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = RetryLayer::<TestRequest, TestError>::builder()
                .max_attempts(3)
                .fixed_backoff(Duration::from_millis(100))
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

fn bench_rate_limiter(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("ratelimiter_permits_available", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = RateLimiterLayer::builder()
                .limit_for_period(1000)
                .refresh_period(Duration::from_secs(1))
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

fn bench_cache(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("cache_hit", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = CacheLayer::builder()
                .max_size(100)
                .ttl(Duration::from_secs(60))
                .key_extractor(|req: &TestRequest| req.0)
                .build();
            let mut service = layer.layer(BaselineService);

            // Prime the cache
            let _ = service.ready().await.unwrap().call(TestRequest(42)).await;

            // Measure cache hit
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

fn bench_time_limiter(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("timelimiter_completes_quickly", |b| {
        b.to_async(&runtime).iter(|| async {
            let layer = TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_secs(30))
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

fn bench_composition_simple(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("composition_circuit_breaker_and_bulkhead", |b| {
        b.to_async(&runtime).iter(|| async {
            let cb_layer = CircuitBreakerLayer::<TestResponse, TestError>::builder()
                .failure_rate_threshold(0.5)
                .build();
            let bh_config = BulkheadLayer::builder().max_concurrent_calls(100).build();

            let mut service = cb_layer.layer(
                ServiceBuilder::new()
                    .layer(bh_config)
                    .service(BaselineService),
            );

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
    benches,
    bench_baseline,
    bench_circuit_breaker,
    bench_bulkhead,
    bench_retry,
    bench_rate_limiter,
    bench_cache,
    bench_time_limiter,
    bench_composition_simple
);
criterion_main!(benches);
