//! Tower `Service` contract regression for `WeightedRouter`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.
//!
//! `WeightedRouter::poll_ready` readies every backend before returning
//! `Poll::Ready(Ok(()))`, so the backend chosen by the selector in `call` is
//! guaranteed to be a readied instance. This regression test holds two
//! `StatefulInner`-backed backends and exercises the readied-receiver path
//! across multiple `ready().call(...)` cycles.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_router::WeightedRouter;

#[tokio::test]
async fn router_drives_readied_instance() {
    let mut router = WeightedRouter::builder()
        .route(StatefulInner::new(), 1)
        .route(StatefulInner::new(), 1)
        .build();

    for _ in 0..6 {
        let _ = router.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn router_composes_with_concurrency_limit() {
    let mut router = WeightedRouter::builder()
        .route(ConcurrencyLimit::new(StatefulInner::new(), 8), 1)
        .route(ConcurrencyLimit::new(StatefulInner::new(), 8), 1)
        .build();

    for _ in 0..6 {
        let _ = router.ready().await.unwrap().call(()).await;
    }
}
