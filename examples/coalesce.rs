//! Coalesce (singleflight) example for request deduplication
//!
//! This example demonstrates the coalesce pattern for deduplicating
//! concurrent identical requests - only one request executes while
//! others wait for its result.
//! Run with: cargo run --example coalesce

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_coalesce::CoalesceLayer;

#[derive(Debug, Clone)]
struct MyError(String);

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MyError {}

#[tokio::main]
async fn main() {
    println!("=== Coalesce (Singleflight) Example ===\n");

    // Example 1: Basic coalescing
    println!("--- Example 1: Basic Coalescing ---");
    println!("Multiple concurrent requests with same key share one execution\n");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst);
            println!("  Backend called for '{}' (call #{})", req, n + 1);
            // Simulate slow operation
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, MyError>(format!("Response for: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn 5 concurrent requests with the same key
    let mut handles = vec![];
    for i in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            let result = svc
                .ready()
                .await
                .unwrap()
                .call("same-key".to_string())
                .await;
            println!("  Request {} got: {:?}", i + 1, result);
            result
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    println!(
        "\nTotal backend calls: {} (5 requests, 1 execution)\n",
        call_count.load(Ordering::SeqCst)
    );

    // Example 2: Different keys execute separately
    println!("--- Example 2: Different Keys ---");
    println!("Requests with different keys execute independently\n");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst);
            println!("  Backend called for '{}' (call #{})", req, n + 1);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, MyError>(format!("Response for: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    let mut handles = vec![];
    for i in 0..3 {
        let mut svc = service.clone();
        let key = format!("key-{}", i);
        handles.push(tokio::spawn(async move {
            let result = svc.ready().await.unwrap().call(key).await;
            println!("  Got: {:?}", result);
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    println!(
        "\nTotal backend calls: {} (3 different keys)\n",
        call_count.load(Ordering::SeqCst)
    );

    // Example 3: Error propagation
    println!("--- Example 3: Error Propagation ---");
    println!("Errors are propagated to all waiting requests\n");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            println!("  Backend called - will return error");
            tokio::time::sleep(Duration::from_millis(50)).await;
            Err::<String, _>(MyError("Something went wrong".to_string()))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    let mut handles = vec![];
    for i in 0..3 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            let result = svc
                .ready()
                .await
                .unwrap()
                .call("error-key".to_string())
                .await;
            println!("  Request {} got error: {:?}", i + 1, result.is_err());
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    println!(
        "\nTotal backend calls: {} (1 call, 3 errors propagated)\n",
        call_count.load(Ordering::SeqCst)
    );

    // Example 4: Subsequent requests after completion
    println!("--- Example 4: Subsequent Requests ---");
    println!("After completion, new requests execute fresh\n");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst);
            println!("  Backend call #{} for '{}'", n + 1, req);
            Ok::<_, MyError>(format!("Response #{} for: {}", n + 1, req))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // First request
    let result = service.ready().await.unwrap().call("key".to_string()).await;
    println!("  First: {:?}", result);

    // Second request (after first completes) - executes fresh
    let result = service.ready().await.unwrap().call("key".to_string()).await;
    println!("  Second: {:?}", result);

    println!(
        "\nTotal backend calls: {} (2 sequential requests)\n",
        call_count.load(Ordering::SeqCst)
    );

    // Example 5: Custom key extraction
    println!("--- Example 5: Custom Key Extraction ---");
    println!("Coalesce by a field within the request\n");

    #[derive(Clone)]
    struct UserRequest {
        user_id: u64,
        action: String,
    }

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: UserRequest| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            println!("  Backend: user_id={}, action={}", req.user_id, req.action);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, MyError>(format!("Profile for user {}", req.user_id))
        }
    });

    // Coalesce by user_id only (ignoring action)
    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &UserRequest| req.user_id))
        .service(service);

    let mut handles = vec![];

    // Same user_id, different actions - should coalesce
    for action in ["view", "refresh", "load"] {
        let mut svc = service.clone();
        let req = UserRequest {
            user_id: 123,
            action: action.to_string(),
        };
        handles.push(tokio::spawn(async move {
            let result = svc.ready().await.unwrap().call(req).await;
            println!("  Action '{}' got: {:?}", action, result);
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    println!(
        "\nTotal backend calls: {} (3 requests for same user, 1 execution)\n",
        call_count.load(Ordering::SeqCst)
    );

    println!("=== Done ===");
}
