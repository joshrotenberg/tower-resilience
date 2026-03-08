//! Outlier detection middleware for Tower services.
//!
//! Outlier detection tracks per-instance health on live traffic and ejects
//! unhealthy instances from a fleet. This is complementary to circuit breaker:
//!
//! | | Circuit Breaker | Outlier Detection |
//! |---|---|---|
//! | Trigger | Failure *rate* over sliding window | *Consecutive* errors |
//! | Scope | Per-service, isolated | Fleet-aware (`max_ejection_percent`) |
//! | Detection speed | Needs `minimum_calls` before evaluating | Catches hard-down immediately |
//! | Recovery | Half-open state with probe calls | Time-based automatic recovery |
//!
//! # Quick Start
//!
//! ```rust
//! use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};
//! use tower::{ServiceBuilder, service_fn};
//! use std::time::Duration;
//!
//! // Create a shared detector for the fleet
//! let detector = OutlierDetector::new()
//!     .max_ejection_percent(50)
//!     .base_ejection_duration(Duration::from_secs(30));
//!
//! // Register instances
//! detector.register("backend-1", 5);  // eject after 5 consecutive errors
//! detector.register("backend-2", 5);
//!
//! // Create per-instance layers sharing the same detector
//! let layer1 = OutlierDetectionLayer::builder()
//!     .detector(detector.clone())
//!     .instance_name("backend-1")
//!     .build();
//!
//! let layer2 = OutlierDetectionLayer::builder()
//!     .detector(detector.clone())
//!     .instance_name("backend-2")
//!     .build();
//!
//! // Apply to services
//! let svc1 = ServiceBuilder::new()
//!     .layer(layer1)
//!     .service(service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) }));
//! ```
//!
//! # Backpressure vs Error Mode
//!
//! By default, ejected instances return `Pending` from `poll_ready()`, which
//! causes Tower load balancers to route around them. For cases where you want
//! an explicit error, use `.error_on_ejection()`:
//!
//! ```rust
//! # use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};
//! # let detector = OutlierDetector::new();
//! # detector.register("backend-1", 5);
//! let layer = OutlierDetectionLayer::builder()
//!     .detector(detector)
//!     .instance_name("backend-1")
//!     .error_on_ejection()
//!     .build();
//! ```

/// Configuration types for outlier detection.
pub mod config;
/// Shared fleet-level outlier detector state.
pub mod detector;
/// Error types for outlier detection.
pub mod error;
/// Event types emitted by outlier detection.
pub mod events;
/// Tower `Layer` implementation for outlier detection.
pub mod layer;
/// Tower `Service` implementation for outlier detection.
pub mod service;
/// Ejection strategies (e.g., consecutive errors).
pub mod strategy;

pub use config::{OutlierDetectionConfig, OutlierDetectionConfigBuilder};
pub use detector::OutlierDetector;
pub use error::{OutlierDetectionError, OutlierDetectionServiceError};
pub use events::OutlierDetectionEvent;
pub use layer::OutlierDetectionLayer;
pub use service::OutlierDetectionService;
pub use strategy::{ConsecutiveErrors, EjectionStrategy};
