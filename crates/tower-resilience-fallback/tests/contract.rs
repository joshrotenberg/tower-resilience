//! Tower `Service` contract regression for `Fallback`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_fallback::FallbackLayer;

#[tokio::test]
async fn fallback_drives_readied_instance() {
    let layer = FallbackLayer::<(), (), std::convert::Infallible>::value(());
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
