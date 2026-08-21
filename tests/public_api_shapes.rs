//! Compile-time fixtures for public API shapes that have changed in 0.13.

use std::convert::Infallible;
use std::future::{Ready, ready};
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tower_resilience::{
    adaptive, bulkhead, circuitbreaker, core, outlier, ratelimiter, reconnect, retry,
};

#[derive(Clone)]
struct ReadyService;

impl Service<()> for ReadyService {
    type Response = ();
    type Error = Infallible;
    type Future = Ready<Result<(), Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: ()) -> Self::Future {
        ready(Ok(()))
    }
}

fn assert_named_outlier_future<S>(_service: &S)
where
    S: Service<
            (),
            Future = outlier::OutlierDetectionFuture<
                Ready<Result<(), Infallible>>,
                core::DefaultClassifier,
            >,
        >,
{
}

#[test]
fn fallible_builders_keep_their_result_shapes_through_the_facade() {
    let _: Result<core::AimdController, core::aimd::AimdConfigError> =
        core::AimdController::new(core::AimdConfig::new());
    let _: Result<adaptive::Aimd, core::aimd::AimdConfigError> = adaptive::Aimd::builder().build();
    let _: Result<adaptive::Vegas, adaptive::VegasConfigError> =
        adaptive::Vegas::new(10, 1, 100, 3, 6);
    let _: Result<adaptive::Vegas, adaptive::VegasConfigError> = adaptive::Vegas::builder().build();
    let _: Result<bulkhead::BulkheadLayer, bulkhead::BulkheadConfigError> =
        bulkhead::BulkheadLayer::builder().build();
    let _: Result<circuitbreaker::CircuitBreakerLayer, circuitbreaker::CircuitBreakerConfigError> =
        circuitbreaker::CircuitBreakerLayer::builder().build();
    let _: Result<ratelimiter::RateLimiterLayer, ratelimiter::RateLimiterConfigError> =
        ratelimiter::RateLimiterLayer::builder().build();
    let _: Result<outlier::OutlierDetectionLayer, outlier::OutlierDetectionConfigError> =
        outlier::OutlierDetectionLayer::builder().build();

    // These concrete builder types are returned through type inference from a
    // public method but are not all rendered by cargo-public-api. Keep source
    // fixtures for them in addition to the text snapshots.
    let _: Result<Arc<dyn retry::RetryBudget>, retry::RetryBudgetConfigError> =
        retry::RetryBudgetBuilder::new().token_bucket().build();
    let _: Result<Arc<dyn retry::RetryBudget>, retry::RetryBudgetConfigError> =
        retry::RetryBudgetBuilder::new().aimd().build();
}

#[test]
fn fallible_backoff_constructors_keep_their_result_shapes_through_the_facade() {
    use std::time::Duration;

    let _: Result<retry::ExponentialBackoff, retry::BackoffConfigError> =
        retry::ExponentialBackoff::new(Duration::from_millis(10)).multiplier(2.0);
    let _: Result<retry::ExponentialRandomBackoff, retry::BackoffConfigError> =
        retry::ExponentialRandomBackoff::new(Duration::from_millis(10), 0.5);
    let _: Result<reconnect::ReconnectPolicy, retry::BackoffConfigError> =
        reconnect::ReconnectPolicy::exponential_random(
            Duration::from_millis(10),
            Duration::from_secs(1),
            0.5,
        );
}

#[test]
fn build_with_handle_returns_an_observable_tuple_through_the_facade() {
    let (bulkhead_layer, bulkhead_handle) = bulkhead::BulkheadLayer::builder()
        .build_with_handle()
        .unwrap();
    let (circuitbreaker_layer, circuitbreaker_handle) =
        circuitbreaker::CircuitBreakerLayer::builder()
            .build_with_handle()
            .unwrap();
    let (ratelimiter_layer, ratelimiter_handle) = ratelimiter::RateLimiterLayer::builder()
        .build_with_handle()
        .unwrap();

    let _ = (
        bulkhead_layer,
        bulkhead_handle,
        circuitbreaker_layer,
        circuitbreaker_handle,
        ratelimiter_layer,
        ratelimiter_handle,
    );
}

#[test]
fn facade_exposes_the_named_outlier_service_future() {
    let detector = outlier::OutlierDetector::new();
    detector.register("api", 1);
    let layer = outlier::OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("api")
        .build()
        .unwrap();
    let service = layer.layer(ReadyService);

    assert_named_outlier_future(&service);
}

#[test]
fn common_layers_remain_available_from_the_facade_prelude() {
    use tower_resilience::prelude::*;

    let _: BulkheadLayer = BulkheadLayer::builder().build().unwrap();
    let _: CircuitBreakerLayer = CircuitBreakerLayer::builder().build().unwrap();
    let _: RateLimiterLayer = RateLimiterLayer::builder().build().unwrap();
}
