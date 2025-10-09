//! HTTP server with resilience patterns
//!
//! This example demonstrates server-side resilience patterns:
//! - Rate limiting per client to prevent abuse
//! - Bulkhead for isolating expensive operations
//! - Timeout on request handlers to prevent resource exhaustion
//!
//! Run with: cargo run --example server_api

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceBuilder, ServiceExt, service_fn};
use tower_resilience_bulkhead::BulkheadConfig;
use tower_resilience_ratelimiter::RateLimiterConfig;
use tower_resilience_timelimiter::TimeLimiterConfig;

/// HTTP request (simplified)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Request {
    path: String,
    client_ip: SocketAddr,
}

/// HTTP response
#[derive(Debug, Clone)]
struct Response {
    status: u16,
    body: String,
}

/// Service error
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ServiceError {
    RateLimited,
    BulkheadFull,
    Timeout,
    Internal(String),
}

impl From<tower_resilience_bulkhead::BulkheadError> for ServiceError {
    fn from(err: tower_resilience_bulkhead::BulkheadError) -> Self {
        match err {
            tower_resilience_bulkhead::BulkheadError::Timeout => ServiceError::Timeout,
            tower_resilience_bulkhead::BulkheadError::BulkheadFull { .. } => {
                ServiceError::BulkheadFull
            }
        }
    }
}

impl<E> From<tower_resilience_timelimiter::TimeLimiterError<E>> for ServiceError {
    fn from(_: tower_resilience_timelimiter::TimeLimiterError<E>) -> Self {
        ServiceError::Timeout
    }
}

impl From<tower_resilience_ratelimiter::RateLimiterError> for ServiceError {
    fn from(_: tower_resilience_ratelimiter::RateLimiterError) -> Self {
        ServiceError::RateLimited
    }
}

type ServiceResult = Result<Response, ServiceError>;

/// Simulated API handler
async fn api_handler(req: Request) -> ServiceResult {
    println!("[Handler] Processing request: {}", req.path);

    match req.path.as_str() {
        // Fast endpoint
        "/health" => {
            sleep(Duration::from_millis(10)).await;
            Ok(Response {
                status: 200,
                body: "OK".to_string(),
            })
        }

        // Expensive computation endpoint
        "/report" => {
            println!("[Handler] Generating expensive report...");
            sleep(Duration::from_millis(500)).await;
            Ok(Response {
                status: 200,
                body: "Report data...".to_string(),
            })
        }

        // Medium-cost endpoint
        "/users" => {
            sleep(Duration::from_millis(100)).await;
            Ok(Response {
                status: 200,
                body: r#"[{"id":1,"name":"Alice"}]"#.to_string(),
            })
        }

        _ => Ok(Response {
            status: 404,
            body: "Not found".to_string(),
        }),
    }
}

#[tokio::main]
async fn main() {
    println!("=== HTTP Server Resilience Example ===\n");

    println!("--- Scenario 1: Rate Limiting ---");
    scenario_rate_limiting().await;

    println!("\n--- Scenario 2: Bulkhead for Expensive Operations ---");
    scenario_bulkhead().await;

    println!("\n--- Scenario 3: Full Server Stack (Rate Limit + Bulkhead + Timeout) ---");
    scenario_full_server_stack().await;
}

async fn scenario_rate_limiting() {
    // Simple handler
    let base_service = service_fn(|req: Request| async move { api_handler(req).await });

    // Configure rate limiter: 3 requests per second
    let rate_limiter = RateLimiterConfig::builder()
        .limit_for_period(3) // Allow 3 requests
        .refresh_period(Duration::from_secs(1)) // Per second
        .timeout_duration(Duration::from_millis(100)) // Wait up to 100ms for permit
        .on_permit_acquired(|wait_duration| {
            println!(
                "[Rate Limiter] Request permitted (waited {:?})",
                wait_duration
            );
        })
        .on_permit_rejected(|_wait_duration| {
            println!("[Rate Limiter] Request rejected - rate limit exceeded");
        })
        .build();

    let mut service = rate_limiter.layer(base_service);

    let client_ip: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Make 5 requests rapidly - last 2 should be rate limited
    for i in 1..=5 {
        let req = Request {
            path: "/users".to_string(),
            client_ip,
        };

        println!("\n[Client] Request {} to /users", i);
        match service.ready().await.unwrap().call(req).await {
            Ok(resp) => println!("[Client] Response: {} - {}", resp.status, resp.body),
            Err(e) => println!("[Client] Error: {:?}", e),
        }

        // Small delay between requests
        sleep(Duration::from_millis(50)).await;
    }

    println!("\n[Note] Requests 4-5 were likely rate limited");
}

async fn scenario_bulkhead() {
    let request_count = Arc::new(AtomicU32::new(0));

    // Handler that tracks concurrent executions
    let base_service = service_fn(move |req: Request| {
        let count = request_count.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            println!("[Handler] Request #{} started: {}", count, req.path);
            let result = api_handler(req).await;
            println!("[Handler] Request #{} completed", count);
            result
        }
    });

    // Configure bulkhead: max 2 concurrent requests to expensive endpoint
    let bulkhead = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(200)))
        .name("report-endpoint")
        .on_call_permitted(|current| {
            println!("[Bulkhead] Call permitted (current: {})", current);
        })
        .on_call_rejected(|max| {
            println!(
                "[Bulkhead] Call rejected - max {} concurrent calls reached",
                max
            );
        })
        .build();

    let service = bulkhead.layer(base_service);

    let client_ip: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Spawn 4 concurrent requests to the expensive /report endpoint
    let mut handles = vec![];

    for i in 1..=4 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move {
            let req = Request {
                path: "/report".to_string(),
                client_ip,
            };

            println!("\n[Client] Request {} starting", i);
            match svc.ready().await.unwrap().call(req).await {
                Ok(resp) => println!("[Client] Request {} completed: {}", i, resp.status),
                Err(e) => println!("[Client] Request {} failed: {:?}", i, e),
            }
        });
        handles.push(handle);

        // Stagger the requests slightly
        sleep(Duration::from_millis(50)).await;
    }

    // Wait for all requests to complete
    for handle in handles {
        let _ = handle.await;
    }

    println!("\n[Note] Only 2 requests run concurrently, others wait or get rejected");
}

async fn scenario_full_server_stack() {
    let request_count = Arc::new(AtomicU32::new(0));

    // Build server stack with timeout and bulkhead
    // Note: Complex multi-layer stacks can hit trait bound limitations
    // In practice, you might apply layers at different architectural points
    let service = ServiceBuilder::new()
        // 1. Timeout - prevent runaway handlers
        .layer(
            TimeLimiterConfig::builder()
                .timeout_duration(Duration::from_secs(2))
                .cancel_running_future(true)
                .build(),
        )
        // 2. Bulkhead - isolate expensive operations
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(3)
                .max_wait_duration(Some(Duration::from_millis(500)))
                .name("api-server")
                .on_call_permitted(|current| {
                    println!("[Bulkhead] Call permitted (current: {})", current);
                })
                .on_call_rejected(|max| {
                    println!("[Bulkhead] Call rejected (max: {})", max);
                })
                .build(),
        )
        .service_fn(move |req: Request| {
            let count = request_count.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                println!("[Handler] Processing request #{}: {}", count, req.path);
                api_handler(req).await
            }
        });

    let mut service = service;
    let client_ip: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Simulate various traffic patterns
    let requests = vec![
        ("/health", 1),
        ("/users", 1),
        ("/report", 2), // More expensive
        ("/health", 1),
        ("/users", 1),
        ("/report", 2),
    ];

    for (path, count) in requests {
        for i in 1..=count {
            let req = Request {
                path: path.to_string(),
                client_ip,
            };

            println!("\n[Client] Request to {} (iteration {})", path, i);
            match service.ready().await.unwrap().call(req).await {
                Ok(resp) => {
                    println!("[Client] Success: {} - {}", resp.status, resp.body);
                }
                Err(e) => {
                    println!("[Client] Failed: {:?}", e);
                }
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    println!("\n=== Summary ===");
    println!("This example showed server-side resilience patterns:");
    println!("1. Timeout to prevent runaway request handlers");
    println!("2. Bulkhead to limit concurrent expensive operations");
    println!("\nIn production, you would:");
    println!("- Apply rate limiting at the network/proxy layer");
    println!("- Set different bulkheads for different endpoint groups");
    println!("- Configure timeouts based on endpoint SLAs");
    println!("- Monitor rejection rates and adjust thresholds");
    println!("- Add metrics/tracing for observability");
    println!("\nNote: Complex multi-layer stacks may require manual composition");
    println!("      or applying layers at different architectural points");
}
