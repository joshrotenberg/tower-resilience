//! Chaos engineering layer for Tower services.
//!
//! This crate provides a chaos engineering layer that can inject failures and latency
//! into Tower services for testing resilience patterns. It's designed to help you verify
//! that your circuit breakers, retries, timeouts, and other resilience mechanisms work
//! correctly under adverse conditions.
//!
//! # Features
//!
//! - **Error Injection**: Inject errors at a configurable rate
//! - **Latency Injection**: Add random delays to requests
//! - **Deterministic Testing**: Use seeds for reproducible chaos
//! - **Event System**: Monitor chaos injection via event listeners
//! - **Composable**: Works with all other tower-resilience patterns
//!
//! # Safety
//!
//! **WARNING**: This layer is intended for testing and development only. Never use it in
//! production environments. Consider using feature flags or environment checks to ensure
//! chaos layers are only enabled in non-production environments.
//!
//! # Basic Example
//!
//! ## Latency-Only Chaos (no type parameters needed!)
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_resilience_chaos::ChaosLayer;
//! use std::time::Duration;
//!
//! # async fn example() {
//! // No type parameters required for latency-only chaos!
//! let chaos = ChaosLayer::builder()
//!     .name("api-chaos")
//!     .latency_rate(0.2)  // 20% of requests delayed
//!     .min_latency(Duration::from_millis(50))
//!     .max_latency(Duration::from_millis(200))
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(chaos)
//!     .service_fn(|req: String| async move {
//!         Ok::<String, std::io::Error>(format!("Response to: {}", req))
//!     });
//! # }
//! ```
//!
//! ## Error Injection (types inferred from closure)
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_resilience_chaos::ChaosLayer;
//! use std::time::Duration;
//!
//! # async fn example() {
//! // Types inferred from the error_fn closure signature
//! let chaos = ChaosLayer::builder()
//!     .name("api-chaos")
//!     .error_rate(0.1)  // 10% of requests fail
//!     .error_fn(|_req: &String| {
//!         std::io::Error::new(std::io::ErrorKind::Other, "chaos error!")
//!     })
//!     .latency_rate(0.2)  // 20% of remaining requests delayed
//!     .min_latency(Duration::from_millis(50))
//!     .max_latency(Duration::from_millis(200))
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(chaos)
//!     .service_fn(|req: String| async move {
//!         Ok::<String, std::io::Error>(format!("Response to: {}", req))
//!     });
//! # }
//! ```
//!
//! # Testing Circuit Breakers
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use tower_resilience_chaos::ChaosLayer;
//! use tower_resilience_circuitbreaker::CircuitBreakerLayer;
//! use std::time::Duration;
//!
//! # async fn example() {
//! // Create a chaos layer that fails 60% of requests
//! // Types inferred from closure signature
//! let chaos = ChaosLayer::builder()
//!     .name("circuit-breaker-test")
//!     .error_rate(0.6)
//!     .error_fn(|_req: &String| {
//!         std::io::Error::new(std::io::ErrorKind::Other, "simulated failure")
//!     })
//!     .build();
//!
//! // Wrap with a circuit breaker
//! let circuit_breaker = CircuitBreakerLayer::builder()
//!     .name("test-breaker")
//!     .failure_rate_threshold(0.5)
//!     .sliding_window_size(10)
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(circuit_breaker)
//!     .layer(chaos)
//!     .service_fn(|req: String| async move {
//!         Ok::<String, std::io::Error>(req)
//!     });
//!
//! // Make requests - circuit breaker should open after ~5 failures
//! # }
//! ```
//!
//! # Deterministic Testing
//!
//! Use a seed for reproducible chaos injection:
//!
//! ```rust
//! use tower_resilience_chaos::ChaosLayer;
//!
//! # async fn example() {
//! let chaos = ChaosLayer::builder()
//!     .error_rate(0.5)
//!     .error_fn(|_req: &()| {
//!         std::io::Error::new(std::io::ErrorKind::Other, "chaos")
//!     })
//!     .seed(42)  // Same seed = same sequence of failures
//!     .build();
//!
//! // Running the same test multiple times will produce the same results
//! # }
//! ```
//!
//! # Event Monitoring
//!
//! Track chaos injection with event listeners:
//!
//! ```rust
//! use tower_resilience_chaos::ChaosLayer;
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let errors = Arc::new(AtomicUsize::new(0));
//! let latencies = Arc::new(AtomicUsize::new(0));
//!
//! let e = errors.clone();
//! let l = latencies.clone();
//!
//! let chaos = ChaosLayer::builder()
//!     .error_rate(0.1)
//!     .error_fn(|_req: &()| {
//!         std::io::Error::new(std::io::ErrorKind::Other, "chaos")
//!     })
//!     .latency_rate(0.2)
//!     .on_error_injected(move || {
//!         e.fetch_add(1, Ordering::SeqCst);
//!     })
//!     .on_latency_injected(move |delay: Duration| {
//!         l.fetch_add(1, Ordering::SeqCst);
//!     })
//!     .build();
//!
//! // After running tests, check counters
//! println!("Errors injected: {}", errors.load(Ordering::SeqCst));
//! println!("Latencies injected: {}", latencies.load(Ordering::SeqCst));
//! # }
//! ```
//!
//! # Latency Injection Only
//!
//! Test timeout handling without errors (no type parameters needed!):
//!
//! ```rust
//! use tower_resilience_chaos::ChaosLayer;
//! use std::time::Duration;
//!
//! # async fn example() {
//! // No type parameters required for latency-only chaos!
//! let chaos = ChaosLayer::builder()
//!     .latency_rate(0.5)  // 50% of requests delayed
//!     .min_latency(Duration::from_millis(100))
//!     .max_latency(Duration::from_millis(500))
//!     .build();
//!
//! // Use with TimeLimiter to test timeout behavior
//! # }
//! ```

pub mod config;
pub mod events;
pub mod layer;
pub mod service;

pub use config::{
    ChaosConfig, ChaosConfigBuilder, ChaosConfigBuilderWithRate, CustomErrorFn, ErrorInjector,
    NoErrorInjection,
};
pub use events::ChaosEvent;
pub use layer::ChaosLayer;
pub use service::Chaos;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Layer, Service, ServiceExt};

    #[tokio::test]
    async fn test_no_chaos_passes_through() {
        // Latency-only chaos - no type parameters needed!
        let chaos = ChaosLayer::builder().build();

        let mut service = chaos.layer(tower::service_fn(|req: String| async move {
            Ok::<String, ()>(format!("echo: {}", req))
        }));

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(response, "echo: test");
    }

    #[tokio::test]
    async fn test_error_injection_with_seed() {
        // Error injection - types inferred from closure
        let chaos = ChaosLayer::builder()
            .error_rate(1.0) // Always fail
            .error_fn(|_req: &String| "chaos error")
            .seed(42)
            .build();

        let mut service = chaos.layer(tower::service_fn(|req: String| async move {
            Ok::<String, &'static str>(req)
        }));

        let result = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "chaos error");
    }

    #[tokio::test]
    async fn test_latency_injection() {
        // Latency-only chaos - no type parameters needed!
        let chaos = ChaosLayer::builder()
            .latency_rate(1.0) // Always add latency
            .min_latency(Duration::from_millis(50))
            .max_latency(Duration::from_millis(50))
            .seed(42)
            .build();

        let mut service = chaos.layer(tower::service_fn(|req: String| async move {
            Ok::<String, ()>(req)
        }));

        let start = std::time::Instant::now();
        let _response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        let elapsed = start.elapsed();

        // Should have at least 50ms delay (with some tolerance for Windows)
        assert!(
            elapsed.as_millis() >= 40,
            "Expected at least 40ms, got {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn test_event_listeners() {
        let error_count = Arc::new(AtomicUsize::new(0));
        let latency_count = Arc::new(AtomicUsize::new(0));
        let pass_count = Arc::new(AtomicUsize::new(0));

        let e = error_count.clone();
        let l = latency_count.clone();
        let p = pass_count.clone();

        // Latency-only chaos with event listeners - no type parameters needed!
        let chaos = ChaosLayer::builder()
            .latency_rate(0.0)
            .on_error_injected(move || {
                e.fetch_add(1, Ordering::SeqCst);
            })
            .on_latency_injected(move |_delay| {
                l.fetch_add(1, Ordering::SeqCst);
            })
            .on_passed_through(move || {
                p.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        let mut service = chaos.layer(tower::service_fn(|req: String| async move {
            Ok::<String, &'static str>(req)
        }));

        // Make a request that should pass through
        let _response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        assert_eq!(error_count.load(Ordering::SeqCst), 0);
        assert_eq!(latency_count.load(Ordering::SeqCst), 0);
        assert_eq!(pass_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_deterministic_behavior() {
        // Create two services with the same seed
        // Types inferred from closure
        let make_service = || {
            let chaos = ChaosLayer::builder()
                .error_rate(0.5)
                .error_fn(|_req: &String| "error")
                .seed(123)
                .build();

            chaos.layer(tower::service_fn(|req: String| async move {
                Ok::<String, &'static str>(req)
            }))
        };

        let mut service1 = make_service();
        let mut service2 = make_service();

        // Make the same sequence of requests
        for i in 0..10 {
            let req = format!("req{}", i);
            let result1 = service1.ready().await.unwrap().call(req.clone()).await;
            let result2 = service2.ready().await.unwrap().call(req).await;

            // Results should be identical
            assert_eq!(result1.is_ok(), result2.is_ok());
        }
    }
}
