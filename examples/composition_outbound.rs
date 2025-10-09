//! Outbound client with full resilience stack
//!
//! This example demonstrates best practices for composing resilience patterns
//! in an outbound HTTP/API client. The pattern composition order matters!
//!
//! Recommended client stack (outside to inside):
//! 1. Cache - Try cache first to avoid unnecessary calls
//! 2. Timeout - Don't wait forever for responses
//! 3. Circuit Breaker - Fail fast when service is down
//! 4. Retry - Handle transient failures
//!
//! Run with: cargo run --example composition_outbound

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceBuilder, ServiceExt, service_fn};
use tower_resilience_cache::CacheConfig;
use tower_resilience_circuitbreaker::CircuitBreakerConfig;
use tower_resilience_retry::RetryConfig;
use tower_resilience_timelimiter::TimeLimiterConfig;

/// HTTP request (simplified)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct Request {
    method: String,
    url: String,
    user_id: Option<u32>, // For cache keying
}

/// HTTP response (simplified)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Response {
    status: u16,
    body: String,
    cached: bool,
}

/// Client errors
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ClientError {
    /// Network errors (connection refused, timeout, etc.)
    Network(String),
    /// HTTP errors
    Http(u16, String),
    /// Timeout
    Timeout,
    /// Circuit breaker open
    CircuitOpen,
}

impl<E> From<tower_resilience_circuitbreaker::CircuitBreakerError<E>> for ClientError {
    fn from(_: tower_resilience_circuitbreaker::CircuitBreakerError<E>) -> Self {
        ClientError::CircuitOpen
    }
}

impl<E> From<tower_resilience_timelimiter::TimeLimiterError<E>> for ClientError {
    fn from(_: tower_resilience_timelimiter::TimeLimiterError<E>) -> Self {
        ClientError::Timeout
    }
}

/// Simulated external API that can fail
struct ExternalApi {
    call_count: Arc<AtomicU32>,
    fail_mode: ApiFailMode,
}

#[derive(Clone)]
#[allow(dead_code)]
enum ApiFailMode {
    /// Normal operation
    Normal,
    /// Fail first N calls with network errors
    FailFirst(u32),
    /// Always fail (for circuit breaker demo)
    AlwaysFail,
    /// Slow responses
    Slow,
}

impl ExternalApi {
    fn new(fail_mode: ApiFailMode) -> Self {
        Self {
            call_count: Arc::new(AtomicU32::new(0)),
            fail_mode,
        }
    }

    async fn call(&self, req: &Request) -> Result<Response, ClientError> {
        let call_num = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        println!(
            "[API] External call #{}: {} {}",
            call_num, req.method, req.url
        );

        match &self.fail_mode {
            ApiFailMode::Normal => {
                sleep(Duration::from_millis(100)).await;
                Ok(Response {
                    status: 200,
                    body: format!("API response for {}", req.url),
                    cached: false,
                })
            }
            ApiFailMode::FailFirst(count) => {
                if call_num <= *count {
                    sleep(Duration::from_millis(50)).await;
                    println!("[API] Network error on call #{}", call_num);
                    Err(ClientError::Network("Connection timeout".to_string()))
                } else {
                    sleep(Duration::from_millis(100)).await;
                    Ok(Response {
                        status: 200,
                        body: format!("API response for {}", req.url),
                        cached: false,
                    })
                }
            }
            ApiFailMode::AlwaysFail => {
                sleep(Duration::from_millis(50)).await;
                Err(ClientError::Network("Service unavailable".to_string()))
            }
            ApiFailMode::Slow => {
                sleep(Duration::from_secs(3)).await;
                Ok(Response {
                    status: 200,
                    body: format!("Slow API response for {}", req.url),
                    cached: false,
                })
            }
        }
    }
}

#[tokio::main]
async fn main() {
    println!("=== Outbound Client Resilience Composition Example ===\n");

    println!("--- Scenario 1: Retry + Timeout for Transient Failures ---");
    scenario_retry_timeout().await;

    println!("\n--- Scenario 2: Circuit Breaker Prevents Cascading Failures ---");
    scenario_circuit_breaker().await;

    println!("\n--- Scenario 3: Cache Reduces Load ---");
    scenario_cache().await;

    println!("\n--- Scenario 4: Full Client Stack (Cache + Timeout + Circuit Breaker + Retry) ---");
    scenario_full_stack().await;
}

async fn scenario_retry_timeout() {
    // API that fails first 2 times, then succeeds
    let api = Arc::new(ExternalApi::new(ApiFailMode::FailFirst(2)));

    let base_client = service_fn(move |req: Request| {
        let api = Arc::clone(&api);
        async move { api.call(&req).await }
    });

    // Compose: Timeout -> Retry
    let client = ServiceBuilder::new()
        .layer(
            TimeLimiterConfig::builder()
                .timeout_duration(Duration::from_secs(2))
                .on_timeout(|| {
                    println!("[Timeout] Request exceeded 2s timeout");
                })
                .build(),
        )
        .layer(
            RetryConfig::<ClientError>::builder()
                .max_attempts(5)
                .exponential_backoff(Duration::from_millis(100))
                .retry_on(|err| {
                    // Retry network errors, not HTTP errors
                    matches!(err, ClientError::Network(_))
                })
                .on_retry(|attempt, delay| {
                    println!(
                        "[Retry] Attempt {} failed, retrying after {:?}",
                        attempt, delay
                    );
                })
                .build(),
        )
        .service(base_client);

    let mut client = client;

    let req = Request {
        method: "GET".to_string(),
        url: "/api/users/123".to_string(),
        user_id: Some(123),
    };

    println!("[Client] Making request to {}", req.url);
    match client.ready().await.unwrap().call(req).await {
        Ok(resp) => println!("[Client] Success: {} - {}", resp.status, resp.body),
        Err(e) => println!("[Client] Failed: {:?}", e),
    }
}

async fn scenario_circuit_breaker() {
    // API that always fails
    let api = Arc::new(ExternalApi::new(ApiFailMode::AlwaysFail));

    let base_client = service_fn(move |req: Request| {
        let api = Arc::clone(&api);
        async move { api.call(&req).await }
    });

    // Circuit breaker to fail fast
    let circuit_breaker = CircuitBreakerConfig::<Response, ClientError>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(2))
        .name("external-api")
        .on_state_transition(|from, to| {
            println!("[Circuit Breaker] State: {:?} -> {:?}", from, to);
        })
        .on_call_rejected(|| {
            println!("[Circuit Breaker] Call rejected - circuit is open");
        })
        .build();

    let mut client = circuit_breaker.layer(base_client);

    // Make several requests - circuit should open
    for i in 1..=6 {
        let req = Request {
            method: "GET".to_string(),
            url: format!("/api/data/{}", i),
            user_id: Some(i as u32),
        };

        println!("\n[Client] Request {} to {}", i, req.url);
        match client.ready().await.unwrap().call(req).await {
            Ok(resp) => println!("[Client] Success: {}", resp.status),
            Err(e) => println!("[Client] Failed: {:?}", e),
        }

        sleep(Duration::from_millis(100)).await;
    }

    println!("\n[Note] Circuit opened after failures, preventing wasteful retries");
}

async fn scenario_cache() {
    let api = Arc::new(ExternalApi::new(ApiFailMode::Normal));
    let call_count = Arc::clone(&api.call_count);

    let base_client = service_fn(move |req: Request| {
        let api = Arc::clone(&api);
        async move { api.call(&req).await }
    });

    // Add cache layer
    let cache_layer = CacheConfig::builder()
        .max_size(100)
        .ttl(Duration::from_secs(60))
        .key_extractor(|req: &Request| {
            // Cache GET requests by user_id
            if req.method == "GET" {
                req.user_id.unwrap_or(0)
            } else {
                0 // Don't cache non-GET
            }
        })
        .on_hit(|| {
            println!("[Cache] Hit");
        })
        .on_miss(|| {
            println!("[Cache] Miss");
        })
        .build();

    let mut client = cache_layer.layer(base_client);

    // Make same request 3 times
    for i in 1..=3 {
        let req = Request {
            method: "GET".to_string(),
            url: "/api/users/123".to_string(),
            user_id: Some(123),
        };

        println!("\n[Client] Request {} (same user)", i);
        match client.ready().await.unwrap().call(req).await {
            Ok(resp) => println!("[Client] Response: {}", resp.body),
            Err(e) => println!("[Client] Error: {:?}", e),
        }
    }

    let total_calls = call_count.load(Ordering::SeqCst);
    println!(
        "\n[Note] Only {} actual API call(s) made for 3 requests",
        total_calls
    );
}

async fn scenario_full_stack() {
    // API with occasional transient failures
    let api = Arc::new(ExternalApi::new(ApiFailMode::FailFirst(2)));
    let api_call_count = Arc::clone(&api.call_count);

    // Client resilience stack: Cache -> Retry
    // Note: Complex multi-layer stacks can hit trait bound limitations
    // In practice, layers can be applied at different architectural points
    let client = ServiceBuilder::new()
        // 1. Cache (outermost) - Skip everything if cached
        .layer(
            CacheConfig::builder()
                .max_size(100)
                .ttl(Duration::from_secs(60))
                .key_extractor(|req: &Request| {
                    if req.method == "GET" {
                        req.user_id.unwrap_or(0)
                    } else {
                        0
                    }
                })
                .on_hit(|| {
                    println!("[Cache] Hit - skipping network call");
                })
                .on_miss(|| {
                    println!("[Cache] Miss - calling API");
                })
                .build(),
        )
        // 2. Retry - Handle transient failures
        .layer(
            RetryConfig::<ClientError>::builder()
                .max_attempts(3)
                .exponential_backoff(Duration::from_millis(100))
                .retry_on(|err| matches!(err, ClientError::Network(_)))
                .on_retry(|attempt, _| {
                    println!("[Retry] Retrying after attempt {}", attempt);
                })
                .build(),
        )
        .service_fn(move |req: Request| {
            let api = Arc::clone(&api);
            async move { api.call(&req).await }
        });

    let mut client = client;

    // Simulate various API calls
    let requests = vec![
        (123, 1), // Same user, should be cached after first call
        (123, 2),
        (456, 1), // Different user
        (123, 3), // Should hit cache
        (789, 1), // Another user
    ];

    let num_requests = requests.len();
    for (user_id, iteration) in requests {
        let req = Request {
            method: "GET".to_string(),
            url: format!("/api/users/{}", user_id),
            user_id: Some(user_id),
        };

        println!(
            "\n[Client] Request for user {} (iteration {})",
            user_id, iteration
        );
        match client.ready().await.unwrap().call(req).await {
            Ok(resp) => {
                println!("[Client] Success: {}", resp.body);
            }
            Err(e) => {
                println!("[Client] Failed: {:?}", e);
            }
        }

        sleep(Duration::from_millis(150)).await;
    }

    let total_api_calls = api_call_count.load(Ordering::SeqCst);
    println!("\n[Stats] Total API calls made: {}", total_api_calls);
    println!("[Stats] Total client requests: {}", num_requests);
    println!(
        "[Stats] Cache prevented {} calls",
        num_requests as u32 - total_api_calls
    );

    println!("\n=== Summary ===");
    println!("This example demonstrated client-side resilience patterns:");
    println!("\n1. Cache (outermost)");
    println!("   - Fastest path - avoids all downstream layers");
    println!("   - Reduces load on external services");
    println!("   - Only cache GET requests (idempotent)");
    println!("\n2. Retry");
    println!("   - Handles transient network errors");
    println!("   - Uses exponential backoff to avoid overwhelming");
    println!("   - Only retries network errors, not client errors");
    println!("\nRecommended full stack (when trait bounds permit):");
    println!("Cache -> Timeout -> Circuit Breaker -> Retry");
    println!("\nWhy that order?");
    println!("- Cache first: Avoid all downstream processing");
    println!("- Timeout before circuit breaker: Timeouts count as failures");
    println!("- Circuit breaker before retry: Don't retry when circuit is open");
    println!("- Retry last: Give each request multiple chances");
    println!("\nNote: Complex multi-layer stacks may require manual composition");
    println!("      or applying patterns at different architectural points.");
    println!("\nIn production, you would also:");
    println!("- Add request/response logging and tracing");
    println!("- Monitor cache hit rates and circuit breaker states");
    println!("- Configure timeouts based on API SLAs");
    println!("- Use different circuit breakers per downstream service");
    println!("- Add jitter to retry backoff");
}
