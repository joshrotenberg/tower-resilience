//! Fallback example for graceful degradation
//!
//! This example demonstrates the fallback pattern for providing
//! alternative responses when the primary service fails.
//! Run with: cargo run --example fallback

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt};
use tower_resilience_fallback::FallbackLayer;

#[derive(Debug, Clone)]
struct ServiceError {
    code: u16,
    message: String,
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceError {}

#[tokio::main]
async fn main() {
    println!("=== Fallback Example ===\n");

    // Example 1: Static value fallback
    println!("--- Example 1: Static Value Fallback ---");
    println!("Service fails, returns a default value instead");
    println!();

    let service = tower::service_fn(|req: String| async move {
        println!("  Primary service called with: {}", req);
        println!("  Primary service returning error");
        Err::<String, _>(ServiceError {
            code: 500,
            message: "Internal error".to_string(),
        })
    });

    let layer =
        FallbackLayer::<String, String, ServiceError>::value("default response".to_string());

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request-1".to_string())
        .await;
    println!("Result: {:?}\n", result);

    // Example 2: Dynamic fallback from error
    println!("--- Example 2: Fallback From Error ---");
    println!("Fallback response is computed from the error");
    println!();

    let service = tower::service_fn(|req: String| async move {
        println!("  Primary service called with: {}", req);
        Err::<String, _>(ServiceError {
            code: 503,
            message: "Service unavailable".to_string(),
        })
    });

    let layer = FallbackLayer::<String, String, ServiceError>::from_error(|e| {
        format!("Fallback: service returned {} - {}", e.code, e.message)
    });

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request-2".to_string())
        .await;
    println!("Result: {:?}\n", result);

    // Example 3: Fallback using request context
    println!("--- Example 3: Fallback From Request and Error ---");
    println!("Fallback response uses both the original request and error");
    println!();

    let service = tower::service_fn(|req: String| async move {
        println!("  Primary service called with: {}", req);
        Err::<String, _>(ServiceError {
            code: 404,
            message: "Not found".to_string(),
        })
    });

    let layer = FallbackLayer::<String, String, ServiceError>::from_request_error(|req, e| {
        format!("Request '{}' failed: {} - {}", req, e.code, e.message)
    });

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("get-user-123".to_string())
        .await;
    println!("Result: {:?}\n", result);

    // Example 4: Service-based fallback
    println!("--- Example 4: Service-Based Fallback ---");
    println!("Fallback to a backup service when primary fails");
    println!();

    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let counter = Arc::clone(&counter);
        async move {
            let count = counter.fetch_add(1, Ordering::SeqCst);
            println!("  Primary service called (attempt {})", count + 1);
            Err::<String, _>(ServiceError {
                code: 500,
                message: "Primary failed".to_string(),
            })
        }
    });

    let layer = FallbackLayer::<String, String, ServiceError>::service(|req| {
        Box::pin(async move {
            println!("  Backup service handling request");
            Ok(format!("Backup handled: {}", req))
        })
    });

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("important-request".to_string())
        .await;
    println!("Result: {:?}\n", result);

    // Example 5: Selective fallback (only for certain errors)
    println!("--- Example 5: Selective Fallback ---");
    println!("Only use fallback for 5xx errors, let 4xx pass through");
    println!();

    let service = tower::service_fn(|req: String| async move {
        println!("  Primary service called with: {}", req);
        if req.contains("client") {
            Err::<String, _>(ServiceError {
                code: 400,
                message: "Bad request".to_string(),
            })
        } else {
            Err(ServiceError {
                code: 503,
                message: "Service unavailable".to_string(),
            })
        }
    });

    let layer = FallbackLayer::<String, String, ServiceError>::builder()
        .value("fallback for server errors".to_string())
        .handle(|e: &ServiceError| e.code >= 500)
        .build();

    let mut service = layer.layer(service);

    // Server error - uses fallback
    println!("Request 'server-error' (503):");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("server-error".to_string())
        .await;
    println!("Result: {:?}", result);

    // Client error - passes through (no fallback)
    println!("\nRequest 'client-error' (400):");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("client-error".to_string())
        .await;
    println!("Result: {:?}\n", result);

    // Example 6: Success case (no fallback needed)
    println!("--- Example 6: No Fallback When Primary Succeeds ---");
    println!();

    let service = tower::service_fn(|req: String| async move {
        println!("  Primary service called - returning success");
        Ok::<_, ServiceError>(format!("Primary response for: {}", req))
    });

    let layer =
        FallbackLayer::<String, String, ServiceError>::value("this won't be used".to_string());

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("successful-request".to_string())
        .await;
    println!("Result: {:?}\n", result);

    println!("=== Done ===");
}
