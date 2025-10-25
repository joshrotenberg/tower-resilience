//! Proactive health checking for resources with intelligent selection strategies.
//!
//! This module provides a health-aware wrapper that continuously monitors the health
//! of multiple resources (Redis connections, HTTP clients, databases, etc.) and
//! intelligently selects healthy resources when requested.
//!
//! # Key Distinction
//!
//! - **Circuit Breaker** (reactive): Responds to failures after they happen
//! - **Health Check** (proactive): Continuously monitors health to prevent failures
//!
//! These patterns complement each other perfectly!
//!
//! # Examples
//!
//! ```rust
//! use tower_resilience_healthcheck::{HealthCheckWrapper, HealthStatus, HealthChecker};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Define a health checker for your resource type
//! struct DbHealthChecker;
//!
//! impl HealthChecker<String> for DbHealthChecker {
//!     async fn check(&self, db: &String) -> HealthStatus {
//!         // Your health check logic here
//!         HealthStatus::Healthy
//!     }
//! }
//!
//! // Create wrapper with multiple resources
//! let wrapper = HealthCheckWrapper::builder()
//!     .with_context("primary-db".to_string(), "primary")
//!     .with_context("secondary-db".to_string(), "secondary")
//!     .with_checker(DbHealthChecker)
//!     .with_interval(Duration::from_secs(5))
//!     .build();
//!
//! // Start background health checking
//! wrapper.start().await;
//!
//! // Get a healthy resource
//! if let Some(db) = wrapper.get_healthy().await {
//!     println!("Using healthy database: {}", db);
//! }
//! # Ok(())
//! # }
//! ```

mod checker;
mod config;
mod context;
mod selector;
mod wrapper;

pub use checker::HealthChecker;
pub use config::{HealthCheckConfig, HealthCheckConfigBuilder};
pub use context::{HealthCheckedContext, HealthDetail};
pub use selector::{SelectionStrategy, Selector};
pub use wrapper::{HealthCheckWrapper, HealthCheckWrapperBuilder};

/// Health status of a monitored resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Resource is healthy and ready to use
    Healthy,

    /// Resource is degraded but still functional (e.g., slow but working)
    Degraded,

    /// Resource is unhealthy and should not be used
    Unhealthy,

    /// Health status is unknown (not yet checked or check failed)
    Unknown,
}

impl HealthStatus {
    /// Check if the status indicates the resource is usable (Healthy or Degraded).
    pub fn is_usable(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Degraded)
    }

    /// Check if the status indicates the resource is healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }
}

// For atomic storage
impl From<HealthStatus> for u8 {
    fn from(status: HealthStatus) -> u8 {
        match status {
            HealthStatus::Healthy => 0,
            HealthStatus::Degraded => 1,
            HealthStatus::Unhealthy => 2,
            HealthStatus::Unknown => 3,
        }
    }
}

impl From<u8> for HealthStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => HealthStatus::Healthy,
            1 => HealthStatus::Degraded,
            2 => HealthStatus::Unhealthy,
            _ => HealthStatus::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_is_usable() {
        assert!(HealthStatus::Healthy.is_usable());
        assert!(HealthStatus::Degraded.is_usable());
        assert!(!HealthStatus::Unhealthy.is_usable());
        assert!(!HealthStatus::Unknown.is_usable());
    }

    #[test]
    fn test_health_status_is_healthy() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Degraded.is_healthy());
        assert!(!HealthStatus::Unhealthy.is_healthy());
        assert!(!HealthStatus::Unknown.is_healthy());
    }

    #[test]
    fn test_health_status_conversions() {
        assert_eq!(u8::from(HealthStatus::Healthy), 0);
        assert_eq!(u8::from(HealthStatus::Degraded), 1);
        assert_eq!(u8::from(HealthStatus::Unhealthy), 2);
        assert_eq!(u8::from(HealthStatus::Unknown), 3);

        assert_eq!(HealthStatus::from(0_u8), HealthStatus::Healthy);
        assert_eq!(HealthStatus::from(1_u8), HealthStatus::Degraded);
        assert_eq!(HealthStatus::from(2_u8), HealthStatus::Unhealthy);
        assert_eq!(HealthStatus::from(3_u8), HealthStatus::Unknown);
        assert_eq!(HealthStatus::from(99_u8), HealthStatus::Unknown);
    }
}
