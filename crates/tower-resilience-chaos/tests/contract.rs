//! Tower `Service` contract regression for `Chaos`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::{Service, ServiceExt};
use tower_resilience_chaos::ChaosLayer;
use tower_resilience_core::testing::StatefulInner;

#[tokio::test]
async fn chaos_drives_readied_instance() {
    // No chaos injection -- we only care about the readiness contract.
    let layer = ChaosLayer::builder().build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
