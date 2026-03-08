//! Integration tests for ResilienceErrorLayer and unified error composition.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_core::{ResilienceError, ResilienceErrorLayer};
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

#[derive(Debug, Clone)]
struct AppError(String);

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AppError {}

#[tokio::test]
async fn test_single_layer_unified() {
    let svc = tower::service_fn(|req: String| async move {
        Ok::<_, AppError>(req.to_uppercase())
    });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(10).build();
    let layer = ResilienceErrorLayer::<_, AppError>::new(bulkhead);

    let mut svc = ServiceBuilder::new().layer(layer).service(svc);

    let resp: Result<String, _> = svc.ready().await.unwrap().call("hello".into()).await;
    assert_eq!(resp.unwrap(), "HELLO");
}

#[tokio::test]
async fn test_bulkhead_error_converts() {
    let svc = tower::service_fn(|req: String| async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok::<_, AppError>(req)
    });

    let bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Duration::from_millis(1))
        .build();
    let layer = ResilienceErrorLayer::<_, AppError>::new(bulkhead);

    let mut svc = ServiceBuilder::new().layer(layer).service(svc);

    // Fill the single slot
    let mut svc2 = svc.clone();
    let _handle = tokio::spawn(async move {
        let _ = ServiceExt::<String>::ready(&mut svc2)
            .await
            .unwrap()
            .call("busy".into())
            .await;
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    let err: ResilienceError<AppError> = svc
        .ready()
        .await
        .unwrap()
        .call("rejected".into())
        .await
        .unwrap_err();
    assert!(err.is_timeout() || err.is_bulkhead_full());
}

#[tokio::test]
async fn test_circuit_breaker_error_converts() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count2 = call_count.clone();

    let svc = tower::service_fn(move |_req: String| {
        let count = call_count2.fetch_add(1, Ordering::SeqCst);
        async move {
            if count < 10 {
                Err::<String, AppError>(AppError("fail".into()))
            } else {
                Ok("ok".into())
            }
        }
    });

    let cb = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .build();
    let layer = ResilienceErrorLayer::<_, AppError>::new(cb);

    let mut svc = ServiceBuilder::new().layer(layer).service(svc);

    // Send failures to trip the circuit breaker
    for _ in 0..5 {
        let _: Result<String, _> = svc.ready().await.unwrap().call("req".into()).await;
    }

    let result: Result<String, ResilienceError<AppError>> =
        svc.ready().await.unwrap().call("req".into()).await;
    match result {
        Err(ref e) if e.is_circuit_open() => {}
        Err(ref e) if e.is_application() => {}
        other => panic!("Expected circuit open or app error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_two_layers_stacked() {
    let svc = tower::service_fn(|req: String| async move {
        Ok::<_, AppError>(req.to_uppercase())
    });

    let bulkhead = ResilienceErrorLayer::<_, AppError>::new(
        BulkheadLayer::builder().max_concurrent_calls(50).build(),
    );
    let cb = ResilienceErrorLayer::<_, AppError>::new(
        CircuitBreakerLayer::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(100)
            .build(),
    );

    let mut svc = ServiceBuilder::new()
        .layer(cb)
        .layer(bulkhead)
        .service(svc);

    let resp: Result<String, _> = svc.ready().await.unwrap().call("hello".into()).await;
    assert_eq!(resp.unwrap(), "HELLO");
}

#[tokio::test]
async fn test_three_layers_stacked() {
    let svc = tower::service_fn(|req: String| async move {
        Ok::<_, AppError>(req.to_uppercase())
    });

    let rl = ResilienceErrorLayer::<_, AppError>::new(
        RateLimiterLayer::builder()
            .limit_for_period(1000)
            .refresh_period(Duration::from_secs(1))
            .build(),
    );
    let bulkhead = ResilienceErrorLayer::<_, AppError>::new(
        BulkheadLayer::builder().max_concurrent_calls(50).build(),
    );
    let timeout = ResilienceErrorLayer::<_, AppError>::new(
        TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_secs(5))
            .build(),
    );

    let mut svc = ServiceBuilder::new()
        .layer(rl)
        .layer(bulkhead)
        .layer(timeout)
        .service(svc);

    let resp: Result<String, _> = svc.ready().await.unwrap().call("hello".into()).await;
    assert_eq!(resp.unwrap(), "HELLO");
}

#[tokio::test]
async fn test_unified_extension_trait() {
    use tower_resilience_core::UnifiedErrors;

    let svc = tower::service_fn(|req: String| async move {
        Ok::<_, AppError>(req.to_uppercase())
    });

    let mut svc = ServiceBuilder::new()
        .layer(
            BulkheadLayer::builder()
                .max_concurrent_calls(50)
                .build()
                .unified::<AppError>(),
        )
        .service(svc);

    let resp: Result<String, _> = svc.ready().await.unwrap().call("hello".into()).await;
    assert_eq!(resp.unwrap(), "HELLO");
}

#[tokio::test]
async fn test_application_error_preserved() {
    let svc = tower::service_fn(|_req: String| async {
        Err::<String, AppError>(AppError("db connection failed".into()))
    });

    let bulkhead = ResilienceErrorLayer::<_, AppError>::new(
        BulkheadLayer::builder().max_concurrent_calls(10).build(),
    );

    let mut svc = ServiceBuilder::new().layer(bulkhead).service(svc);

    let err: ResilienceError<AppError> = svc
        .ready()
        .await
        .unwrap()
        .call("req".into())
        .await
        .unwrap_err();
    assert!(err.is_application());
    let app_err = err.application_error().unwrap();
    assert_eq!(app_err.0, "db connection failed");
}

#[tokio::test]
async fn test_error_display_distinguishes_failure_modes() {
    let timeout_err: ResilienceError<AppError> = ResilienceError::Timeout {
        layer: "time_limiter",
    };
    assert!(timeout_err.to_string().contains("Timeout"));

    let circuit_err: ResilienceError<AppError> =
        ResilienceError::CircuitOpen { name: Some("api".into()) };
    assert!(circuit_err.to_string().contains("Circuit breaker"));
    assert!(circuit_err.to_string().contains("api"));

    let bulkhead_err: ResilienceError<AppError> = ResilienceError::BulkheadFull {
        concurrent_calls: 50,
        max_concurrent: 50,
    };
    assert!(bulkhead_err.to_string().contains("Bulkhead full"));

    let rate_err: ResilienceError<AppError> =
        ResilienceError::RateLimited { retry_after: None };
    assert!(rate_err.to_string().contains("Rate limited"));

    let ejected_err: ResilienceError<AppError> = ResilienceError::InstanceEjected {
        name: "backend-1".into(),
    };
    assert!(ejected_err.to_string().contains("backend-1"));
}
