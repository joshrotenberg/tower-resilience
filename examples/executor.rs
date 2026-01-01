//! Executor Layer Example
//!
//! This example demonstrates how to use the executor layer to delegate
//! request processing to different runtimes or thread pools.
//!
//! Run with: cargo run --example executor

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_executor::ExecutorLayer;

#[derive(Clone)]
struct ComputeService {
    /// Track how many requests are being processed concurrently
    concurrent: Arc<AtomicUsize>,
}

impl ComputeService {
    fn new() -> Self {
        Self {
            concurrent: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Service<u64> for ComputeService {
    type Response = u64;
    type Error = std::io::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, n: u64) -> Self::Future {
        let concurrent = Arc::clone(&self.concurrent);

        Box::pin(async move {
            let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            println!(
                "    [Thread {:?}] Processing n={}, concurrent={}",
                std::thread::current().id(),
                n,
                current
            );

            // Simulate CPU-intensive work
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Compute fibonacci (simplified for demo)
            let result = fib(n);

            concurrent.fetch_sub(1, Ordering::SeqCst);
            Ok(result)
        })
    }
}

fn fib(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => {
            let mut a = 0u64;
            let mut b = 1u64;
            for _ in 2..=n {
                let c = a.saturating_add(b);
                a = b;
                b = c;
            }
            b
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Executor Layer Example ===\n");

    // Example 1: Basic usage with current runtime
    println!("--- Basic Usage ---");
    println!("Using ExecutorLayer::current() to spawn requests as tasks.\n");

    let layer = ExecutorLayer::current();
    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service(ComputeService::new());

    for i in 1..=5 {
        let result = service.ready().await?.call(i * 10).await?;
        println!("  fib({}) = {}", i * 10, result);
    }

    println!();

    // Example 2: Parallel execution
    println!("--- Parallel Execution ---");
    println!("Spawning multiple requests concurrently.\n");

    let layer = ExecutorLayer::current();
    let service = ServiceBuilder::new()
        .layer(layer)
        .service(ComputeService::new());

    let mut handles = vec![];
    for i in 1..=6 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = svc.ready().await.unwrap().call(i * 5).await;
            (i * 5, result, start.elapsed())
        }));
    }

    for handle in handles {
        let (n, result, elapsed) = handle.await?;
        match result {
            Ok(value) => println!("  fib({}) = {} ({:?})", n, value, elapsed),
            Err(e) => println!("  fib({}) failed: {} ({:?})", n, e, elapsed),
        }
    }

    println!();

    // Example 3: Using a dedicated runtime
    println!("--- Dedicated Runtime ---");
    println!("Processing requests on a separate runtime with dedicated threads.\n");

    let compute_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("compute-worker")
        .build()?;

    let layer = ExecutorLayer::new(compute_runtime.handle().clone());
    let service = ServiceBuilder::new()
        .layer(layer)
        .service(ComputeService::new());

    let mut handles = vec![];
    for i in 1..=4 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i * 8).await
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await?;
        match result {
            Ok(value) => println!("  fib({}) = {}", (i + 1) * 8, value),
            Err(e) => println!("  fib({}) failed: {}", (i + 1) * 8, e),
        }
    }

    // Shutdown the dedicated runtime
    compute_runtime.shutdown_timeout(Duration::from_secs(1));

    println!("\n=== Example Complete ===");
    println!("\nKey takeaways:");
    println!("- ExecutorLayer spawns each request as a separate task");
    println!("- Unlike Buffer, requests are processed in parallel, not serially");
    println!("- Use a dedicated runtime for CPU-bound or blocking work");
    println!("- Combine with Bulkhead for bounded parallelism");

    Ok(())
}
