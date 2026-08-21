//! Compile-time regression coverage for wrapping conventional Tower errors.

use tower::BoxError;
use tower::{Layer, Service, service_fn};

fn assert_into_box_error<T>()
where
    T: Into<BoxError>,
{
}

fn assert_service_error_into_box_error<S>(_: &S)
where
    S: Service<()>,
    S::Error: Into<BoxError>,
{
}

#[test]
fn affected_layers_wrap_a_box_error_service() {
    let bulkhead = tower_resilience_bulkhead::BulkheadLayer::builder()
        .max_concurrent_calls(1)
        .build()
        .expect("valid bulkhead");
    let bulkhead_service = bulkhead.layer(service_fn(|()| async {
        Err::<(), BoxError>(std::io::Error::other("inner failure").into())
    }));
    assert_service_error_into_box_error(&bulkhead_service);

    let aimd = tower_resilience_adaptive::Aimd::builder()
        .initial_limit(1)
        .build()
        .expect("valid AIMD configuration");
    let adaptive = tower_resilience_adaptive::AdaptiveLimiterLayer::new(aimd);
    let adaptive_service = adaptive.layer(service_fn(|()| async {
        Err::<(), BoxError>(std::io::Error::other("inner failure").into())
    }));
    assert_service_error_into_box_error(&adaptive_service);
}

#[test]
fn generic_error_wrappers_convert_into_tower_box_error() {
    assert_into_box_error::<tower_resilience_adaptive::AdaptiveError<BoxError>>();
    assert_into_box_error::<tower_resilience_bulkhead::BulkheadServiceError<BoxError>>();
    assert_into_box_error::<tower_resilience_cache::CacheError<BoxError>>();
    assert_into_box_error::<tower_resilience_circuitbreaker::CircuitBreakerError<BoxError>>();
    assert_into_box_error::<tower_resilience_coalesce::CoalesceError<BoxError>>();
    assert_into_box_error::<tower_resilience_core::ResilienceError<BoxError>>();
    assert_into_box_error::<tower_resilience_executor::ExecutorError<BoxError>>();
    assert_into_box_error::<tower_resilience_fallback::FallbackError<BoxError>>();
    assert_into_box_error::<tower_resilience_hedge::HedgeError<BoxError>>();
    assert_into_box_error::<tower_resilience_outlier::OutlierDetectionServiceError<BoxError>>();
    assert_into_box_error::<tower_resilience_ratelimiter::RateLimiterServiceError<BoxError>>();
    assert_into_box_error::<tower_resilience_reconnect::ReconnectError<BoxError, BoxError>>();
    assert_into_box_error::<tower_resilience_router::WeightedRouterError<BoxError>>();
    assert_into_box_error::<tower_resilience_timelimiter::TimeLimiterError<BoxError>>();
}

#[test]
fn typed_inner_error_remains_recoverable() {
    let inner: BoxError = std::io::Error::other("inner failure").into();
    let error = tower_resilience_bulkhead::BulkheadServiceError::Inner(inner);

    assert_eq!(
        error.into_inner().expect("inner error").to_string(),
        "inner failure"
    );
}

#[test]
fn library_owned_error_remains_a_source() {
    use std::error::Error;

    let error: tower_resilience_bulkhead::BulkheadServiceError<BoxError> =
        tower_resilience_bulkhead::BulkheadError::Timeout.into();

    assert_eq!(
        error.source().expect("bulkhead source").to_string(),
        "timeout waiting for bulkhead permit"
    );
}
