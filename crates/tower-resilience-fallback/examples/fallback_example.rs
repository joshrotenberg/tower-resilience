//! Fallback pattern examples.
//!
//! Run with: cargo run --example fallback_example -p tower-resilience-fallback
//!
//! This example demonstrates:
//! - Static value fallback
//! - Dynamic fallback from error
//! - Dynamic fallback from request and error
//! - Service-based fallback
//! - Selective fallback with predicates

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Tower Fallback Example");
    println!("======================\n");

    // Example 1: Static value fallback
    println!("Example 1: Static value fallback");

    let service = tower::service_fn(|_req: String| async move {
        println!("  Service called - returning error");
        Err::<String, _>(ServiceError {
            code: 500,
            message: "Internal error".to_string(),
        })
    });

    let fallback_layer =
        FallbackLayer::<String, String, ServiceError>::value("default response".to_string());

    let mut service = fallback_layer.layer(service);

    let result = service.ready().await?.call("request".to_string()).await?;
    println!("  Result: {}\n", result);

    // Example 2: Fallback from error
    println!("Example 2: Dynamic fallback from error");

    let service = tower::service_fn(|_req: String| async move {
        println!("  Service called - returning error");
        Err::<String, _>(ServiceError {
            code: 503,
            message: "Service unavailable".to_string(),
        })
    });

    let fallback_layer = FallbackLayer::<String, String, ServiceError>::from_error(|e| {
        format!("Fallback due to error {}: {}", e.code, e.message)
    });

    let mut service = fallback_layer.layer(service);

    let result = service.ready().await?.call("request".to_string()).await?;
    println!("  Result: {}\n", result);

    // Example 3: Fallback from request and error
    println!("Example 3: Fallback using request context");

    let service = tower::service_fn(|_req: String| async move {
        println!("  Service called - returning error");
        Err::<String, _>(ServiceError {
            code: 404,
            message: "Not found".to_string(),
        })
    });

    let fallback_layer =
        FallbackLayer::<String, String, ServiceError>::from_request_error(|req, e| {
            format!("Request '{}' failed with {}: {}", req, e.code, e.message)
        });

    let mut service = fallback_layer.layer(service);

    let result = service
        .ready()
        .await?
        .call("get-user-123".to_string())
        .await?;
    println!("  Result: {}\n", result);

    // Example 4: Service-based fallback
    println!("Example 4: Service-based fallback");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            println!("  Primary service called (attempt {})", count + 1);
            Err::<String, _>(ServiceError {
                code: 500,
                message: "Primary failed".to_string(),
            })
        }
    });

    let fallback_layer = FallbackLayer::<String, String, ServiceError>::service(|req| {
        Box::pin(async move {
            println!("  Fallback service called");
            Ok(format!("Fallback handled: {}", req))
        })
    });

    let mut service = fallback_layer.layer(service);

    let result = service
        .ready()
        .await?
        .call("important-request".to_string())
        .await?;
    println!("  Result: {}\n", result);

    // Example 5: Selective fallback with predicate
    println!("Example 5: Selective fallback (only for 5xx errors)");

    let service = tower::service_fn(|req: String| async move {
        println!("  Service called with: {}", req);
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

    let fallback_layer = FallbackLayer::<String, String, ServiceError>::builder()
        .value("fallback for server errors".to_string())
        .handle(|e: &ServiceError| e.code >= 500) // Only handle 5xx errors
        .build();

    let mut service = fallback_layer.layer(service);

    // This should use fallback (503 error)
    println!("  Request 'server-error':");
    let result = service
        .ready()
        .await?
        .call("server-error".to_string())
        .await;
    println!("  Result: {:?}", result);

    // This should NOT use fallback (400 error) - error passes through
    println!("  Request 'client-error':");
    let result = service
        .ready()
        .await?
        .call("client-error".to_string())
        .await;
    println!("  Result: {:?}\n", result);

    // Example 6: Fallback when primary succeeds (no fallback triggered)
    println!("Example 6: No fallback when primary succeeds");

    let service = tower::service_fn(|req: String| async move {
        println!("  Service called - returning success");
        Ok::<_, ServiceError>(format!("Primary response for: {}", req))
    });

    let fallback_layer =
        FallbackLayer::<String, String, ServiceError>::value("this won't be used".to_string());

    let mut service = fallback_layer.layer(service);

    let result = service
        .ready()
        .await?
        .call("successful-request".to_string())
        .await?;
    println!("  Result: {}\n", result);

    Ok(())
}
