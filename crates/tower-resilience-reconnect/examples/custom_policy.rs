use std::time::Duration;
use tower_resilience_reconnect::{
    ExponentialBackoff, FixedInterval, IntervalFunction, ReconnectConfig, ReconnectLayer,
    ReconnectPolicy,
};

/// Example demonstrating different reconnection policies.
///
/// This example shows how to configure various backoff strategies:
/// - Fixed interval
/// - Exponential backoff
/// - Custom interval function
///
/// Custom backoff that increases linearly
struct LinearBackoff {
    initial_delay: Duration,
    increment: Duration,
    max_delay: Duration,
}

impl IntervalFunction for LinearBackoff {
    fn next_interval(&self, attempt: usize) -> Duration {
        let delay = self.initial_delay + self.increment * (attempt as u32);
        std::cmp::min(delay, self.max_delay)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reconnect Layer - Custom Policy Examples\n");

    // Example 1: Fixed interval reconnection
    println!("1. Fixed Interval Policy");
    println!("   Reconnects every 1 second, up to 5 attempts\n");
    let config1 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::Fixed(FixedInterval::new(
            Duration::from_secs(1),
        )))
        .max_attempts(5)
        .build();
    let _layer1 = ReconnectLayer::new(config1);

    // Example 2: Exponential backoff
    println!("2. Exponential Backoff Policy");
    println!("   Starts at 100ms, doubles each attempt, max 10s\n");
    let config2 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100),
            Duration::from_secs(10),
        ))
        .unlimited_attempts()
        .build();
    let _layer2 = ReconnectLayer::new(config2);

    // Example 3: Exponential backoff (using the struct directly)
    println!("3. Exponential Backoff with Custom Settings\n");
    let backoff = ExponentialBackoff::new(Duration::from_millis(50))
        .multiplier(1.5)
        .max_interval(Duration::from_secs(30));
    let config3 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::Exponential(backoff))
        .max_attempts(10)
        .build();
    let _layer3 = ReconnectLayer::new(config3);

    // Example 4: Custom linear backoff
    println!("4. Custom Linear Backoff Policy");
    println!("   Starts at 500ms, increases by 200ms each attempt, max 5s\n");
    let custom_backoff = LinearBackoff {
        initial_delay: Duration::from_millis(500),
        increment: Duration::from_millis(200),
        max_delay: Duration::from_secs(5),
    };
    let config4 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::Custom(std::sync::Arc::new(custom_backoff)))
        .max_attempts(20)
        .build();
    let _layer4 = ReconnectLayer::new(config4);

    // Example 5: No reconnection (fail fast)
    println!("5. No Reconnection Policy");
    println!("   Fails immediately on connection errors\n");
    let config5 = ReconnectConfig::builder()
        .policy(ReconnectPolicy::None)
        .build();
    let _layer5 = ReconnectLayer::new(config5);

    println!("All policy examples created successfully!");
    println!("\nEach policy provides different trade-offs:");
    println!("  - Fixed: Predictable timing, simple");
    println!("  - Exponential: Fast initial retries, backs off over time");
    println!("  - Custom: Full control over backoff behavior");
    println!("  - None: Fail fast for critical errors");

    Ok(())
}
