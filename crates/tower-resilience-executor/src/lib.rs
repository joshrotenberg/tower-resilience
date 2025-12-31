//! Executor delegation layer for Tower services.
//!
//! This crate provides a Tower layer that delegates request processing to an
//! arbitrary executor for parallel processing. Unlike Tower's `Buffer` layer
//! which processes requests serially, this layer spawns each request as a
//! separate task, enabling true parallelism.
//!
//! # Use Cases
//!
//! - **CPU-bound processing**: Parallelize CPU-intensive request handling
//! - **Runtime isolation**: Process requests on a dedicated runtime
//! - **Thread pool delegation**: Use specific thread pools for certain workloads
//! - **Blocking operations**: Offload blocking I/O to dedicated threads
//!
//! # Example
//!
//! ```rust
//! use tower_resilience_executor::{ExecutorLayer, Executor};
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use tokio::runtime::Handle;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a simple service
//! let service = tower::service_fn(|req: String| async move {
//!     Ok::<_, std::convert::Infallible>(format!("Hello, {}!", req))
//! });
//!
//! // Wrap with executor layer using current runtime
//! let mut service = ServiceBuilder::new()
//!     .layer(ExecutorLayer::current())
//!     .service(service);
//!
//! // Make a request - it will be processed on a spawned task
//! let response = service.ready().await?.call("World".to_string()).await?;
//! assert_eq!(response, "Hello, World!");
//! # Ok(())
//! # }
//! ```
//!
//! # Using a Dedicated Runtime
//!
//! ```rust,no_run
//! use tower_resilience_executor::ExecutorLayer;
//! use tower::ServiceBuilder;
//!
//! // Create a dedicated runtime for heavy computation
//! let compute_runtime = tokio::runtime::Builder::new_multi_thread()
//!     .worker_threads(8)
//!     .thread_name("compute")
//!     .build()
//!     .unwrap();
//!
//! // Use the dedicated runtime for request processing
//! let layer = ExecutorLayer::new(compute_runtime.handle().clone());
//! ```
//!
//! # Combining with Bulkhead
//!
//! For bounded parallel execution, combine with a bulkhead layer:
//!
//! ```rust,ignore
//! use tower_resilience_executor::ExecutorLayer;
//! use tower_resilience_bulkhead::BulkheadLayer;
//! use tower::ServiceBuilder;
//!
//! let service = ServiceBuilder::new()
//!     // Limit concurrent requests
//!     .layer(BulkheadLayer::builder().max_concurrent_calls(16).build())
//!     // Execute on dedicated runtime
//!     .layer(ExecutorLayer::current())
//!     .service(tower::service_fn(|_: ()| async { Ok::<_, ()>(()) }));
//! ```
//!
//! # Service Requirements
//!
//! The wrapped service must implement `Clone`. This is necessary because each
//! spawned task needs its own instance of the service. Most Tower services
//! already implement `Clone`, and for those that don't, consider wrapping
//! them with `Buffer` first.

mod executor;
mod layer;
mod service;

pub use executor::{BlockingExecutor, CurrentRuntime, Executor};
pub use layer::{ExecutorLayer, ExecutorLayerBuilder};
pub use service::{ExecutorError, ExecutorFuture, ExecutorService};

#[cfg(test)]
mod tests {
    use super::*;
    use tower::{Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn test_basic_usage() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req * 2) });

        let mut service = ServiceBuilder::new()
            .layer(ExecutorLayer::current())
            .service(service);

        let response = service.ready().await.unwrap().call(21).await.unwrap();
        assert_eq!(response, 42);
    }

    #[tokio::test]
    async fn test_parallel_execution() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let counter = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let max_clone = Arc::clone(&max_concurrent);

        let service = tower::service_fn(move |_req: ()| {
            let counter = Arc::clone(&counter_clone);
            let max_concurrent = Arc::clone(&max_clone);
            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;

                // Update max concurrent if this is higher
                let mut max = max_concurrent.load(Ordering::SeqCst);
                while current > max {
                    match max_concurrent.compare_exchange_weak(
                        max,
                        current,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(m) => max = m,
                    }
                }

                // Simulate some work
                tokio::time::sleep(Duration::from_millis(50)).await;

                counter.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, &str>(())
            }
        });

        let service = ServiceBuilder::new()
            .layer(ExecutorLayer::current())
            .service(service);

        // Spawn multiple concurrent requests
        let mut handles = Vec::new();
        for _ in 0..10 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(()).await.unwrap();
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify we had parallel execution
        assert!(
            max_concurrent.load(Ordering::SeqCst) > 1,
            "Expected parallel execution, but max concurrent was {}",
            max_concurrent.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn test_with_custom_executor() {
        let handle = tokio::runtime::Handle::current();

        let service =
            tower::service_fn(|req: String| async move { Ok::<_, &str>(req.to_uppercase()) });

        let mut service = ServiceBuilder::new()
            .layer(ExecutorLayer::new(handle))
            .service(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("hello".to_string())
            .await
            .unwrap();
        assert_eq!(response, "HELLO");
    }

    #[tokio::test]
    async fn test_error_propagation() {
        let service = tower::service_fn(|_req: ()| async move { Err::<(), _>("service error") });

        let mut service = ServiceBuilder::new()
            .layer(ExecutorLayer::current())
            .service(service);

        let result = service.ready().await.unwrap().call(()).await;
        assert!(matches!(
            result,
            Err(ExecutorError::Service("service error"))
        ));
    }

    #[tokio::test]
    async fn test_builder_pattern() {
        let service = tower::service_fn(|req: i32| async move { Ok::<_, &str>(req) });

        let mut service = ServiceBuilder::new()
            .layer(
                ExecutorLayer::<tokio::runtime::Handle>::builder()
                    .current()
                    .build(),
            )
            .service(service);

        let response = service.ready().await.unwrap().call(42).await.unwrap();
        assert_eq!(response, 42);
    }
}
