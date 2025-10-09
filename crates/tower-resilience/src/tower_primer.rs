//! # Tower Primer
//!
//! A brief introduction to Tower for developers new to the framework.
//!
//! ## What is Tower?
//!
//! [Tower](https://docs.rs/tower) is a library of modular and composable components for
//! building robust networking clients and servers. It provides:
//!
//! - **Service trait** - A unified interface for async request/response operations
//! - **Middleware layers** - Composable transformations applied to services
//! - **Battle-tested patterns** - From Finagle, used in production at scale
//!
//! Tower-resilience builds on Tower to provide resilience patterns as composable middleware.
//!
//! ## Core Concepts
//!
//! ### The Service Trait
//!
//! At the heart of Tower is the [`Service`](tower::Service) trait:
//!
//! ```rust,ignore
//! pub trait Service<Request> {
//!     type Response;
//!     type Error;
//!     type Future: Future<Output = Result<Self::Response, Self::Error>>;
//!
//!     fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
//!     fn call(&mut self, req: Request) -> Self::Future;
//! }
//! ```
//!
//! **Key points:**
//! - `poll_ready()` - Checks if the service is ready to accept a request (backpressure mechanism)
//! - `call()` - Processes the request and returns a future
//! - Generic over `Request` type - Works with HTTP, gRPC, custom protocols
//!
//! ### Layers
//!
//! A [`Layer`](tower::Layer) wraps a service to add behavior:
//!
//! ```rust,ignore
//! pub trait Layer<S> {
//!     type Service;
//!     fn layer(&self, inner: S) -> Self::Service;
//! }
//! ```
//!
//! Layers are like middleware - they intercept requests and responses to add functionality
//! (timeouts, retries, metrics, etc.) without modifying the core service logic.
//!
//! ### ServiceBuilder
//!
//! [`ServiceBuilder`](tower::ServiceBuilder) provides a convenient way to compose multiple layers:
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! let service = ServiceBuilder::new()
//!     .timeout(Duration::from_secs(30))
//!     .concurrency_limit(100)
//!     .service(my_service);
//! ```
//!
//! ## How Services Work
//!
//! ### The Request Lifecycle
//!
//! ```text
//! 1. Check readiness
//!    service.poll_ready() → Poll::Ready(Ok(()))
//!
//! 2. Make request
//!    let future = service.call(request);
//!
//! 3. Await response
//!    let response = future.await?;
//! ```
//!
//! ### Service Cloning
//!
//! **Critical:** Tower services must implement `Clone` to handle concurrent requests.
//!
//! ```rust,no_run
//! # use tower::Service;
//! # async fn example() {
//! # let mut service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
//! // Each task gets its own clone
//! let mut svc1 = service.clone();
//! let mut svc2 = service.clone();
//!
//! tokio::spawn(async move {
//!     let _ = svc1.call(()).await;
//! });
//!
//! tokio::spawn(async move {
//!     let _ = svc2.call(()).await;
//! });
//! # }
//! ```
//!
//! **For shared state**, use `Arc<Mutex<T>>` or similar:
//!
//! ```rust,no_run
//! use std::sync::{Arc, Mutex};
//!
//! #[derive(Clone)]
//! struct MyService {
//!     shared_state: Arc<Mutex<State>>,
//! }
//!
//! # struct State;
//! ```
//!
//! This is why all tower-resilience patterns use `Arc` internally - they maintain shared
//! state (circuit breaker status, rate limiter permits, etc.) that survives clones.
//!
//! ## Why Tower for Resilience?
//!
//! ### 1. Composability
//!
//! Resilience patterns compose naturally as layers:
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! let resilient_client = ServiceBuilder::new()
//!     .timeout(Duration::from_secs(30))        // Tower built-in
//!     .layer(circuit_breaker)                  // Tower-resilience
//!     .layer(retry)                            // Tower-resilience
//!     .service(client);
//! ```
//!
//! ### 2. Type Safety
//!
//! The type system ensures correct composition:
//! - Request/response types must align
//! - Error types must be compatible
//! - Compile-time verification of layer stacks
//!
//! ### 3. Zero-Cost Abstractions
//!
//! Tower's design enables:
//! - Static dispatch (no dynamic dispatch overhead)
//! - Inlining across layers
//! - Minimal runtime overhead (see our [benchmarks](https://github.com/joshrotenberg/tower-resilience#performance))
//!
//! ### 4. Ecosystem
//!
//! Tower is used throughout the Rust async ecosystem:
//! - [Hyper](https://hyper.rs/) - HTTP library
//! - [Tonic](https://github.com/hyperium/tonic) - gRPC framework
//! - [Axum](https://github.com/tokio-rs/axum) - Web framework
//! - Many production services at scale
//!
//! ## Common Pitfalls
//!
//! ### 1. Forgetting poll_ready
//!
//! Always call `poll_ready()` before `call()`:
//!
//! ```rust,no_run
//! # use tower::{Service, ServiceExt};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let mut service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
//! # let request = ();
//! // ❌ Wrong
//! // let response = service.call(request).await;
//!
//! // ✅ Correct
//! service.ready().await?;
//! let response = service.call(request).await?;
//! # Ok(())
//! # }
//! ```
//!
//! **Tip:** Use the `ServiceExt::ready()` helper method.
//!
//! ### 2. Service Cloning Assumptions
//!
//! Don't assume cloned services are independent - they often share state:
//!
//! ```rust,no_run
//! # use std::sync::{Arc, Mutex};
//! # #[derive(Clone)]
//! # struct MyService { state: Arc<Mutex<u32>> }
//! # let service = MyService { state: Arc::new(Mutex::new(0)) };
//! let svc1 = service.clone();
//! let svc2 = service.clone();
//! // svc1 and svc2 share the same underlying state!
//! ```
//!
//! ### 3. Error Type Compatibility
//!
//! When composing layers, error types must be compatible. See our [error integration guide](crate::composition::error_types)
//! or use [`ResilienceError<E>`](tower_resilience_core::ResilienceError).
//!
//! ### 4. Layer Ordering
//!
//! Layers execute **outside-in**. Order matters! See [composition guide](crate::composition::ordering).
//!
//! ## Learning More
//!
//! - [Tower documentation](https://docs.rs/tower) - Comprehensive API docs
//! - [Tower guides](https://github.com/tower-rs/tower/tree/master/guides) - Deep dives into concepts
//! - [Tower examples](https://github.com/tower-rs/tower/tree/master/tower/examples) - Working code samples
//! - [Tokio tutorial](https://tokio.rs/tokio/tutorial) - If you're new to async Rust
//!
//! ## Quick Reference
//!
//! ```rust,no_run
//! use tower::{Service, ServiceExt};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a service
//! let service = tower::service_fn(|req: String| async move {
//!     Ok::<_, std::io::Error>(format!("Hello, {}", req))
//! });
//!
//! // Use the service
//! let mut service = service;
//! let response = service
//!     .ready().await?
//!     .call("World".to_string())
//!     .await?;
//!
//! println!("{}", response); // "Hello, World"
//! # Ok(())
//! # }
//! ```
