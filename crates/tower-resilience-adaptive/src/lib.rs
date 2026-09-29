//! Adaptive concurrency limiter for Tower services.
//!
//! This crate provides a Tower layer that dynamically adjusts concurrency limits
//! based on observed latency and error rates, using AIMD, Vegas, or Gradient2.
//!
//! Unlike static concurrency limits which require manual tuning, adaptive limiters
//! automatically find the optimal concurrency for your downstream services.
//!
//! # Algorithms
//!
//! ## AIMD (Additive Increase Multiplicative Decrease)
//!
//! The classic TCP congestion control algorithm:
//! - On success with low latency: increase limit by a fixed amount
//! - On failure or high latency: decrease limit by a factor (e.g., halve it)
//!
//! This creates a "sawtooth" pattern as it continuously probes for capacity.
//!
//! ## Vegas
//!
//! A more sophisticated algorithm that uses RTT measurements:
//! - Estimates queue depth from RTT variations
//! - Increases limit when queue is small (under-utilized)
//! - Decreases limit when queue is large (congested)
//!
//! Vegas is more stable than AIMD and avoids the sawtooth pattern.
//!
//! ## Gradient2
//!
//! Gradient2 compares the latest RTT with a smoothed long-term RTT baseline.
//! It reduces the limit when latency rises relative to that baseline and grows
//! only when RTT remains healthy while the service is sufficiently utilized.
//! The service supplies the current in-flight count through the additive
//! [`ConcurrencyAlgorithm::record_success_with_load`] hook; existing custom
//! algorithms keep their behavior through its default implementation.
//!
//! # Example
//!
//! ```rust
//! use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a service
//! let service = tower::service_fn(|req: String| async move {
//!     Ok::<_, std::convert::Infallible>(format!("Hello, {}!", req))
//! });
//!
//! // Wrap with adaptive limiter using AIMD
//! let mut service = ServiceBuilder::new()
//!     .layer(AdaptiveLimiterLayer::new(
//!         Aimd::builder()
//!             .initial_limit(10)
//!             .min_limit(1)
//!             .max_limit(100)
//!             .latency_threshold(Duration::from_millis(100))
//!             .build()?
//!     ))
//!     .service(service);
//!
//! // The limit will automatically adjust based on response latency
//! let response = service.ready().await?.call("World".to_string()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Using Vegas Algorithm
//!
//! ```rust,no_run
//! use tower_resilience_adaptive::{AdaptiveLimiterLayer, Vegas};
//! use tower::ServiceBuilder;
//!
//! let layer = AdaptiveLimiterLayer::new(
//!     Vegas::builder()
//!         .initial_limit(10)
//!         .alpha(3)  // Increase when queue < 3
//!         .beta(6)   // Decrease when queue > 6
//!         .build()
//!         .unwrap()
//! );
//! ```
//!
//! # Combining with Other Patterns
//!
//! The adaptive limiter works well with other resilience patterns:
//!
//! ```rust,ignore
//! use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};
//! use tower_resilience_retry::RetryLayer;
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use tower::ServiceBuilder;
//!
//! let service = ServiceBuilder::new()
//!     // Outer: circuit breaker for catastrophic failures
//!     .layer(CircuitBreakerLayer::builder().build())
//!     // Middle: adaptive concurrency limiting
//!     .layer(AdaptiveLimiterLayer::new(Aimd::builder().build()))
//!     // Inner: retry transient failures
//!     .layer(RetryLayer::builder().max_attempts(3).build())
//!     .service(my_service);
//! ```
//!
//! # Prior Art
//!
//! This implementation is inspired by:
//! - [Netflix concurrency-limits](https://github.com/Netflix/concurrency-limits)
//! - [Uber Cinnamon](https://www.uber.com/blog/cinnamon-auto-tuner-adaptive-concurrency-in-the-wild/)
//! - [Vector Adaptive Request Concurrency](https://vector.dev/blog/adaptive-request-concurrency/)

mod algorithm;
mod layer;
mod service;

pub use algorithm::{
    Aimd, AimdBuilder, Algorithm, ConcurrencyAlgorithm, Gradient2, Gradient2Builder,
    Gradient2ConfigError, Vegas, VegasBuilder, VegasConfigError,
};
pub use layer::{AdaptiveLimiterLayer, AdaptiveLimiterLayerBuilder, IntoLayer};
pub use service::{AdaptiveError, AdaptiveFuture, AdaptiveService};
pub use tower_resilience_core::aimd::AimdConfigError;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn test_basic_aimd() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        let mut service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(
                Aimd::builder()
                    .initial_limit(10)
                    .latency_threshold(Duration::from_secs(1))
                    .build()
                    .unwrap(),
            ))
            .service(service);

        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[tokio::test]
    async fn test_limit_increases_on_fast_responses() {
        let service = tower::service_fn(|_req: ()| async {
            // Fast response
            Ok::<_, &str>(())
        });

        let algorithm = Aimd::builder()
            .initial_limit(10)
            .increase_by(1)
            .latency_threshold(Duration::from_secs(1))
            .build()
            .unwrap();

        let initial_limit = algorithm.limit();
        let algorithm = Arc::new(algorithm);

        let mut service = AdaptiveService::new(service, Arc::clone(&algorithm));

        // Make several requests
        for _ in 0..5 {
            service.ready().await.unwrap().call(()).await.unwrap();
        }

        // Limit should have increased
        assert!(algorithm.limit() > initial_limit);
    }

    #[tokio::test]
    async fn test_limit_decreases_on_errors() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |_req: ()| {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            async move {
                if count < 5 {
                    Ok::<_, &str>(())
                } else {
                    Err("error")
                }
            }
        });

        let algorithm = Aimd::builder()
            .initial_limit(20)
            .decrease_factor(0.5)
            .latency_threshold(Duration::from_secs(1))
            .build()
            .unwrap();

        let algorithm = Arc::new(algorithm);
        let mut service = AdaptiveService::new(service, Arc::clone(&algorithm));

        // Make some successful requests
        for _ in 0..5 {
            let _ = service.ready().await.unwrap().call(()).await;
        }

        let limit_before_error = algorithm.limit();

        // Make a failing request
        let _ = service.ready().await.unwrap().call(()).await;

        // Limit should have decreased
        assert!(algorithm.limit() < limit_before_error);
    }

    #[tokio::test]
    async fn test_vegas_basic() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        let mut service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(
                Vegas::builder().initial_limit(10).build().unwrap(),
            ))
            .service(service);

        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[tokio::test]
    async fn gradient2_does_not_grow_for_sequential_light_load() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });
        let algorithm = Arc::new(Gradient2::builder().initial_limit(20).build().unwrap());
        let mut service = AdaptiveService::new(service, Arc::clone(&algorithm));

        for request in 0..30 {
            service.ready().await.unwrap().call(request).await.unwrap();
        }

        assert_eq!(algorithm.limit(), 20);
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let service = tower::service_fn(|_req: ()| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, &str>(())
        });

        let service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(
                Aimd::builder()
                    .initial_limit(5)
                    .latency_threshold(Duration::from_secs(1))
                    .build()
                    .unwrap(),
            ))
            .service(service);

        // Spawn concurrent requests
        let mut handles = vec![];
        for _ in 0..10 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(()).await.unwrap();
            }));
        }

        // All should complete eventually
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_algorithm_enum() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req) });

        // Test with Algorithm enum
        let algorithm = Algorithm::Aimd(Aimd::builder().initial_limit(10).build().unwrap());

        let mut service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(algorithm))
            .service(service);

        let response = service.ready().await.unwrap().call(42).await.unwrap();
        assert_eq!(response, 42);
    }
}
