//! Request coalescing for Tower services.
//!
//! This crate provides a Tower layer that coalesces (deduplicates) concurrent
//! identical requests, ensuring only one request executes while others wait
//! for its result. This prevents "cache stampede" or "thundering herd" problems.
//!
//! # How It Works
//!
//! 1. The first request with a given key begins execution
//! 2. Subsequent requests with the same key wait for the first to complete
//! 3. All waiting requests receive a clone of the result
//! 4. Errors are also propagated to all waiters
//!
//! # Example
//!
//! ```rust
//! use tower_resilience_coalesce::CoalesceLayer;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//!
//! # #[derive(Clone, Hash, Eq, PartialEq)]
//! # struct Request { id: String }
//! # #[derive(Clone)]
//! # struct Response;
//! # #[derive(Debug, Clone)]
//! # struct MyError;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let backend = tower::service_fn(|_req: Request| async { Ok::<_, MyError>(Response) });
//! let service = ServiceBuilder::new()
//!     .layer(CoalesceLayer::new(|req: &Request| req.id.clone()))
//!     .service(backend);
//! # Ok(())
//! # }
//! ```
//!
//! # Use Cases
//!
//! - **Cache refresh protection**: When a cached value expires, multiple requests
//!   may try to refresh it simultaneously. Coalescing ensures only one refresh
//!   happens.
//!
//! - **Expensive computations**: Deduplicate requests for the same expensive
//!   operation (e.g., report generation, ML inference).
//!
//! - **Rate-limited APIs**: Reduce calls to external APIs that have rate limits
//!   by coalescing identical requests.
//!
//! - **Database queries**: Combine identical queries that arrive within a short
//!   window to reduce database load.
//!
//! # Requirements
//!
//! - The key type must implement `Hash + Eq + Clone + Send + Sync`
//! - The response type must implement `Clone`
//! - The error type must implement `Clone`
//!
//! # Prior Art
//!
//! This pattern is also known as:
//! - **Singleflight** (Go's `golang.org/x/sync/singleflight`)
//! - **Request deduplication**
//! - **Request collapsing**

mod config;
mod layer;
mod service;

pub use config::{CoalesceConfig, CoalesceConfigBuilder};
pub use layer::CoalesceLayer;
pub use service::{CoalesceError, CoalesceFuture, CoalesceService};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Service, ServiceBuilder, ServiceExt};

    /// A simple cloneable error type for testing.
    #[derive(Debug, Clone)]
    struct TestError(String);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for TestError {}

    #[tokio::test]
    async fn test_single_request_passes_through() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |req: String| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(format!("response: {}", req))
            }
        });

        let mut service = ServiceBuilder::new()
            .layer(CoalesceLayer::new(|req: &String| req.clone()))
            .service(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response, "response: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_concurrent_requests_coalesce() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |req: String| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                // Simulate slow operation
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok::<_, TestError>(format!("response: {}", req))
            }
        });

        let service = ServiceBuilder::new()
            .layer(CoalesceLayer::new(|req: &String| req.clone()))
            .service(service);

        // Spawn multiple concurrent requests with the same key
        let mut handles = vec![];
        for _ in 0..5 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call("same-key".to_string())
                    .await
            }));
        }

        // All should succeed with the same response
        for handle in handles {
            let result = handle.await.unwrap();
            assert_eq!(result.unwrap(), "response: same-key");
        }

        // But only one actual call was made
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_different_keys_execute_separately() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |req: String| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok::<_, TestError>(format!("response: {}", req))
            }
        });

        let service = ServiceBuilder::new()
            .layer(CoalesceLayer::new(|req: &String| req.clone()))
            .service(service);

        // Spawn requests with different keys
        let mut handles = vec![];
        for i in 0..3 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(format!("key-{}", i)).await
            }));
        }

        for (i, handle) in handles.into_iter().enumerate() {
            let result = handle.await.unwrap();
            assert_eq!(result.unwrap(), format!("response: key-{}", i));
        }

        // Each unique key caused a separate call
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_error_propagates_to_all_waiters() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |_req: String| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Err::<String, _>(TestError("test error".to_string()))
            }
        });

        let service = ServiceBuilder::new()
            .layer(CoalesceLayer::new(|req: &String| req.clone()))
            .service(service);

        // Spawn multiple concurrent requests
        let mut handles = vec![];
        for _ in 0..3 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call("same-key".to_string())
                    .await
            }));
        }

        // All should receive the error
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_err());
        }

        // But only one call was made
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_subsequent_requests_after_completion() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = tower::service_fn(move |req: String| {
            let count = cc.clone();
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(format!("response-{}: {}", n, req))
            }
        });

        let mut service = ServiceBuilder::new()
            .layer(CoalesceLayer::new(|req: &String| req.clone()))
            .service(service);

        // First request
        let r1 = service
            .ready()
            .await
            .unwrap()
            .call("key".to_string())
            .await
            .unwrap();
        assert_eq!(r1, "response-0: key");

        // Second request after first completes - should execute again
        let r2 = service
            .ready()
            .await
            .unwrap()
            .call("key".to_string())
            .await
            .unwrap();
        assert_eq!(r2, "response-1: key");

        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
