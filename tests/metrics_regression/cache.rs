//! Cache metrics regression tests

use super::helpers::*;
use serial_test::serial;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::CacheLayer;

#[tokio::test]
#[serial]
async fn cache_metrics_exist() {
    init_recorder();

    let layer = CacheLayer::builder()
        .name("test_cache")
        .max_size(10)
        .key_extractor(|req: &u64| *req)
        .build();

    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let service = tower::service_fn(move |_: u64| {
        let c = counter_clone.clone();
        async move {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, &'static str>("response")
        }
    });

    let mut service = layer.layer(service);

    // First call - cache miss
    let _ = service.ready().await.unwrap().call(1).await;

    // Second call with same key - cache hit
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify counter metrics
    assert_counter_exists("cache_requests_total");
    assert_metric_has_label("cache_requests_total", "cache", "test_cache");
    assert_metric_has_label("cache_requests_total", "result", "hit");
    assert_metric_has_label("cache_requests_total", "result", "miss");

    // Verify gauge metric
    assert_gauge_exists("cache_size");
    assert_metric_has_label("cache_size", "cache", "test_cache");
}

#[tokio::test]
#[serial]
async fn cache_eviction_metrics() {
    init_recorder();

    let layer = CacheLayer::builder()
        .name("eviction_cache")
        .max_size(2)
        .key_extractor(|req: &u64| *req)
        .build();

    let service = tower::service_fn(|req: u64| async move { Ok::<_, &'static str>(req) });

    let mut service = layer.layer(service);

    // Fill cache beyond capacity to trigger evictions
    for i in 0..5 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Verify eviction counter exists
    assert_counter_exists("cache_evictions_total");
    assert_metric_has_label("cache_evictions_total", "cache", "eviction_cache");
}
