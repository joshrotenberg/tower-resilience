use std::time::Duration;
use tower_resilience_reconnect::{ReconnectConfig, ReconnectPolicy};

#[test]
fn config_builder_default_values() {
    let config = ReconnectConfig::default();

    // Default is exponential backoff with unlimited attempts
    assert!(matches!(config.policy(), ReconnectPolicy::Exponential(_)));
    assert!(config.max_attempts().is_none());
    assert!(config.retry_on_reconnect());
}

#[test]
fn config_builder_custom_values() {
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_secs(1)))
        .max_attempts(5)
        .retry_on_reconnect(false)
        .build();

    assert!(matches!(config.policy(), ReconnectPolicy::Fixed(_)));
    assert_eq!(config.max_attempts(), Some(5));
    assert!(!config.retry_on_reconnect());
}

#[test]
fn config_builder_exponential_backoff() {
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100),
            Duration::from_secs(5),
        ))
        .build();

    assert!(matches!(config.policy(), ReconnectPolicy::Exponential(_)));
}

#[test]
fn config_builder_unlimited_attempts() {
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(100)))
        .unlimited_attempts()
        .build();

    assert!(config.max_attempts().is_none());
}

#[test]
fn config_builder_none_policy() {
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::None)
        .build();

    assert!(matches!(config.policy(), ReconnectPolicy::None));
}
