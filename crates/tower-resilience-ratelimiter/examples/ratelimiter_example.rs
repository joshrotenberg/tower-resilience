//! Rate limiting example with different window types.
//!
//! Run with: cargo run --example ratelimiter_example -p tower-resilience-ratelimiter
//!
//! This example demonstrates:
//! - Fixed window rate limiting (default)
//! - Sliding log rate limiting (precise)
//! - Sliding counter rate limiting (efficient)
//! - The difference in boundary behavior between window types

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterError, RateLimiterLayer, WindowType};

#[tokio::main]
async fn main() {
    println!("=== Tower Resilience Rate Limiter Demo ===\n");

    // Demo 1: Fixed Window (default behavior)
    demo_fixed_window().await;

    println!("\n{}\n", "=".repeat(50));

    // Demo 2: Sliding Log (precise)
    demo_sliding_log().await;

    println!("\n{}\n", "=".repeat(50));

    // Demo 3: Sliding Counter (efficient)
    demo_sliding_counter().await;

    println!("\n{}\n", "=".repeat(50));

    // Demo 4: Boundary behavior comparison
    demo_boundary_comparison().await;
}

async fn demo_fixed_window() {
    println!("1. FIXED WINDOW RATE LIMITING");
    println!("   Resets permits at fixed intervals.");
    println!("   Simple and efficient, but allows bursts at boundaries.\n");

    let permit_count = Arc::new(AtomicUsize::new(0));
    let reject_count = Arc::new(AtomicUsize::new(0));
    let p = Arc::clone(&permit_count);
    let r = Arc::clone(&reject_count);

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::Fixed) // Explicit, but this is the default
        .name("fixed-limiter")
        .on_permit_acquired(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_permit_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = tower::service_fn(|_req: ()| async { Ok::<_, std::convert::Infallible>("OK") });
    let mut svc = ServiceBuilder::new().layer(layer).service(service);

    println!("   Sending 8 requests (limit: 5 per second)...");
    for i in 1..=8 {
        match svc.ready().await.unwrap().call(()).await {
            Ok(_) => println!("   Request {}: permitted", i),
            Err(RateLimiterError::RateLimitExceeded) => println!("   Request {}: rejected", i),
        }
    }

    println!(
        "\n   Result: {} permitted, {} rejected",
        permit_count.load(Ordering::SeqCst),
        reject_count.load(Ordering::SeqCst)
    );
}

async fn demo_sliding_log() {
    println!("2. SLIDING LOG RATE LIMITING");
    println!("   Tracks exact timestamps of each request.");
    println!("   Precise but uses O(n) memory where n = requests in window.\n");

    let permit_count = Arc::new(AtomicUsize::new(0));
    let reject_count = Arc::new(AtomicUsize::new(0));
    let p = Arc::clone(&permit_count);
    let r = Arc::clone(&reject_count);

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::SlidingLog)
        .name("sliding-log-limiter")
        .on_permit_acquired(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_permit_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = tower::service_fn(|_req: ()| async { Ok::<_, std::convert::Infallible>("OK") });
    let mut svc = ServiceBuilder::new().layer(layer).service(service);

    println!("   Sending 8 requests (limit: 5 per second)...");
    for i in 1..=8 {
        match svc.ready().await.unwrap().call(()).await {
            Ok(_) => println!("   Request {}: permitted", i),
            Err(RateLimiterError::RateLimitExceeded) => println!("   Request {}: rejected", i),
        }
    }

    println!(
        "\n   Result: {} permitted, {} rejected",
        permit_count.load(Ordering::SeqCst),
        reject_count.load(Ordering::SeqCst)
    );
}

async fn demo_sliding_counter() {
    println!("3. SLIDING COUNTER RATE LIMITING");
    println!("   Uses weighted averaging between time buckets.");
    println!("   Approximate sliding window with O(1) memory.\n");

    let permit_count = Arc::new(AtomicUsize::new(0));
    let reject_count = Arc::new(AtomicUsize::new(0));
    let p = Arc::clone(&permit_count);
    let r = Arc::clone(&reject_count);

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::SlidingCounter)
        .name("sliding-counter-limiter")
        .on_permit_acquired(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_permit_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = tower::service_fn(|_req: ()| async { Ok::<_, std::convert::Infallible>("OK") });
    let mut svc = ServiceBuilder::new().layer(layer).service(service);

    println!("   Sending 8 requests (limit: 5 per second)...");
    for i in 1..=8 {
        match svc.ready().await.unwrap().call(()).await {
            Ok(_) => println!("   Request {}: permitted", i),
            Err(RateLimiterError::RateLimitExceeded) => println!("   Request {}: rejected", i),
        }
    }

    println!(
        "\n   Result: {} permitted, {} rejected",
        permit_count.load(Ordering::SeqCst),
        reject_count.load(Ordering::SeqCst)
    );
}

async fn demo_boundary_comparison() {
    println!("4. BOUNDARY BEHAVIOR COMPARISON");
    println!("   Demonstrating how window types differ at boundaries.\n");

    // Fixed window allows burst at boundary
    println!("   FIXED WINDOW:");
    {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);

        let layer = RateLimiterLayer::builder()
            .limit_for_period(5)
            .refresh_period(Duration::from_millis(200))
            .timeout_duration(Duration::from_millis(10))
            .window_type(WindowType::Fixed)
            .build();

        let service = tower::service_fn(move |_req: ()| {
            c.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, std::convert::Infallible>("OK") }
        });
        let mut svc = ServiceBuilder::new().layer(layer).service(service);

        // Use all 5 permits
        for _ in 0..5 {
            let _ = svc.ready().await.unwrap().call(()).await;
        }
        println!("   - Used 5 permits");

        // Wait for boundary
        tokio::time::sleep(Duration::from_millis(210)).await;
        println!("   - Waited for window boundary (210ms)");

        // Try 5 more
        for _ in 0..5 {
            let _ = svc.ready().await.unwrap().call(()).await;
        }

        println!(
            "   - Total requests in ~210ms: {} (allows boundary burst)",
            count.load(Ordering::SeqCst)
        );
    }

    println!();

    // Sliding log prevents burst at boundary
    println!("   SLIDING LOG:");
    {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);

        let layer = RateLimiterLayer::builder()
            .limit_for_period(5)
            .refresh_period(Duration::from_millis(200))
            .timeout_duration(Duration::from_millis(10))
            .window_type(WindowType::SlidingLog)
            .build();

        let service = tower::service_fn(move |_req: ()| {
            c.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, std::convert::Infallible>("OK") }
        });
        let mut svc = ServiceBuilder::new().layer(layer).service(service);

        // Use all 5 permits
        for _ in 0..5 {
            let _ = svc.ready().await.unwrap().call(()).await;
        }
        println!("   - Used 5 permits");

        // Wait only 100ms (half window)
        tokio::time::sleep(Duration::from_millis(100)).await;
        println!("   - Waited 100ms (half of 200ms window)");

        // Try 5 more - should fail because requests still in window
        let mut additional = 0;
        for _ in 0..5 {
            if svc.ready().await.unwrap().call(()).await.is_ok() {
                additional += 1;
            }
        }

        println!(
            "   - Additional requests permitted: {} (prevents burst)",
            additional
        );
        println!("   - Total requests: {}", count.load(Ordering::SeqCst));
    }

    println!("\n   Key takeaway:");
    println!("   - Fixed: Simple, allows 2x burst at boundaries");
    println!("   - SlidingLog: Precise, no bursts, O(n) memory");
    println!("   - SlidingCounter: Approximate, smoother than fixed, O(1) memory");
}
