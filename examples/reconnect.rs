//! Reconnect example with exponential backoff
//!
//! This example demonstrates automatic reconnection configuration.
//! Run with: cargo run --example reconnect

use std::time::Duration;
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

#[tokio::main]
async fn main() {
    println!("=== Reconnect Layer Example ===\n");

    // Example 1: Exponential backoff (recommended for most use cases)
    println!("1. Exponential Backoff Policy");
    println!("   Starts at 100ms, doubles each attempt, max 5 seconds");
    let config1 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100), // Start at 100ms
            Duration::from_secs(5),     // Max 5 seconds
        ))
        .max_attempts(10)
        .retry_on_reconnect(true) // Retry the original request after reconnecting
        .build();

    let _layer1 = ReconnectLayer::new(config1);
    println!("   Created layer with 10 max attempts\n");

    // Example 2: Fixed interval (predictable timing)
    println!("2. Fixed Interval Policy");
    println!("   Reconnects every 1 second, up to 5 attempts");
    let config2 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_secs(1)))
        .max_attempts(5)
        .retry_on_reconnect(false) // Don't retry - just reconnect
        .build();

    let _layer2 = ReconnectLayer::new(config2);
    println!("   Created layer with fixed 1s intervals\n");

    // Example 3: Unlimited attempts (use with caution!)
    println!("3. Unlimited Reconnection Attempts");
    println!("   Will keep trying to reconnect forever");
    let config3 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100),
            Duration::from_secs(30),
        ))
        .unlimited_attempts() // Never give up
        .retry_on_reconnect(true)
        .build();

    let _layer3 = ReconnectLayer::new(config3);
    println!("   Created layer with unlimited attempts (use carefully!)\n");

    // Example 4: No reconnection (fail fast)
    println!("4. No Reconnection Policy");
    println!("   Fails immediately on connection errors");
    let config4 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::None)
        .build();

    let _layer4 = ReconnectLayer::new(config4);
    println!("   Created layer that fails fast\n");

    println!("=== Configuration Complete ===\n");

    println!("Usage in Tower ServiceBuilder:");
    println!("  let layer = ReconnectLayer::new(config);");
    println!("  let service = ServiceBuilder::new()");
    println!("      .layer(layer)");
    println!("      .service(make_service);\n");

    println!("Key Features:");
    println!("  - Automatic reconnection on connection failures");
    println!("  - Configurable backoff strategies (exponential, fixed, custom)");
    println!("  - Optional retry of original request after reconnecting");
    println!("  - Connection state tracking and monitoring");
    println!("  - Works with any MakeService implementation\n");

    println!("For a working example with actual connections, see:");
    println!("  crates/tower-resilience-reconnect/examples/basic.rs");
    println!("  crates/tower-resilience-reconnect/examples/custom_policy.rs");
}
