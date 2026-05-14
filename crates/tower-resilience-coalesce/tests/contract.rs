//! Tower `Service` contract regression for `CoalesceService`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.
//!
//! Coalesce's `call` has two paths -- leader (dispatches to inner) and waiter
//! (joins an in-flight request via oneshot). Only the leader path uses the
//! readied receiver. The probe forces every iteration through the leader path
//! by extracting a unique key per call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_coalesce::CoalesceLayer;
use tower_resilience_core::testing::StatefulInner;

fn unique_key_layer(
) -> CoalesceLayer<usize, (), impl Fn(&()) -> usize + Clone + Send + Sync + 'static> {
    let counter = Arc::new(AtomicUsize::new(0));
    CoalesceLayer::new(move |_: &()| counter.fetch_add(1, Ordering::SeqCst))
}

#[tokio::test]
async fn coalesce_leader_drives_readied_instance() {
    let layer = unique_key_layer();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn coalesce_leader_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = unique_key_layer();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
