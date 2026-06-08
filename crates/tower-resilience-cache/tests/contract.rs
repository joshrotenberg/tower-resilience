//! Tower `Service` contract regression for `Cache`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.
//!
//! Cache has two execution paths -- hit (returns immediately, does not touch
//! inner) and miss (dispatches to inner). Only the miss path exercises the
//! readied receiver, so the probe uses a key extractor that produces a fresh
//! key per call to force every iteration through the miss path.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_cache::CacheLayer;
use tower_resilience_core::testing::StatefulInner;

fn unique_key_layer() -> CacheLayer<(), usize> {
    let counter = Arc::new(AtomicUsize::new(0));
    CacheLayer::<(), usize>::builder()
        .max_size(16)
        .ttl(Duration::from_secs(60))
        .key_extractor(move |_: &()| counter.fetch_add(1, Ordering::SeqCst))
        .build()
        .unwrap()
}

#[tokio::test]
async fn cache_miss_drives_readied_instance() {
    let layer = unique_key_layer();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn cache_miss_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = unique_key_layer();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
