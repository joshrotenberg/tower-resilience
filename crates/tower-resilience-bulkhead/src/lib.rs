//! Bulkhead pattern for Tower services.
//!
//! The bulkhead pattern isolates resources to prevent cascading failures.
//! This implementation uses semaphore-based concurrency limiting to control
//! the maximum number of concurrent calls to a service.
//!
//! # Basic Example
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_resilience_bulkhead::BulkheadConfig;
//! use std::time::Duration;
//!
//! # async fn example() {
//! // Create a bulkhead that allows max 10 concurrent calls
//! let layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(10)
//!     .name("my-bulkhead")
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(|req: String| async move {
//!         // Your service logic here
//!         Ok::<_, ()>(req)
//!     });
//! # }
//! ```
//!
//! # Example with Timeout
//!
//! Configure a maximum wait duration for requests when the bulkhead is at capacity:
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_resilience_bulkhead::{BulkheadConfig, BulkheadError};
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(5)
//!     .max_wait_duration(Some(Duration::from_secs(2)))
//!     .name("timeout-bulkhead")
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(|req: String| async move {
//!         Ok::<_, ()>(req)
//!     });
//!
//! // Requests will timeout if they wait more than 2 seconds
//! // for bulkhead capacity
//! # }
//! ```
//!
//! # Example with Event Listeners
//!
//! Monitor bulkhead behavior using event listeners:
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_resilience_bulkhead::BulkheadConfig;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(10)
//!     .name("monitored-bulkhead")
//!     .on_call_permitted(|concurrent| {
//!         println!("Call permitted ({} concurrent)", concurrent);
//!     })
//!     .on_call_rejected(|max| {
//!         println!("Call rejected (max {} concurrent)", max);
//!     })
//!     .on_call_finished(|duration| {
//!         println!("Call finished in {:?}", duration);
//!     })
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(|req: String| async move {
//!         Ok::<_, ()>(req)
//!     });
//! # }
//! ```
//!
//! # Error Handling
//!
//! The bulkhead passes through the inner service's errors directly.
//! Use event listeners to track bulkhead rejections:
//!
//! ```rust
//! use tower_resilience_bulkhead::BulkheadConfig;
//! use tower::ServiceBuilder;
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let rejections = Arc::new(AtomicUsize::new(0));
//! let r = rejections.clone();
//!
//! let layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(5)
//!     .on_call_rejected(move |_| {
//!         r.fetch_add(1, Ordering::SeqCst);
//!     })
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(|req: String| async move {
//!         Ok::<_, ()>(req)
//!     });
//!
//! // Check rejections counter to monitor bulkhead behavior
//! println!("Rejections: {}", rejections.load(Ordering::SeqCst));
//! # }
//! ```

pub mod config;
pub mod error;
pub mod events;
pub mod layer;
pub mod service;

pub use config::{BulkheadConfig, BulkheadConfigBuilder};
pub use error::{BulkheadError, Result};
pub use events::BulkheadEvent;
pub use layer::BulkheadLayer;
pub use service::Bulkhead;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_config_builder_defaults() {
        let _config = BulkheadConfig::builder().build();
        // Layer is built, so we can't inspect config directly
        // This test just ensures the builder works
    }

    #[test]
    fn test_config_builder_with_custom_values() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        let _layer = BulkheadConfig::builder()
            .max_concurrent_calls(5)
            .max_wait_duration(Some(Duration::from_millis(100)))
            .name("test-bulkhead")
            .on_call_permitted(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        // Builder accepts all parameters without panic
    }

    #[test]
    fn test_bulkhead_error_display() {
        let err = BulkheadError::BulkheadFull {
            max_concurrent_calls: 10,
        };
        assert!(err.to_string().contains("10"));

        let err = BulkheadError::Timeout;
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_bulkhead_event_types() {
        use std::time::Instant;
        use tower_resilience_core::events::ResilienceEvent;

        let event = BulkheadEvent::CallPermitted {
            pattern_name: "test".to_string(),
            timestamp: Instant::now(),
            concurrent_calls: 5,
        };
        assert_eq!(event.event_type(), "call_permitted");
        assert_eq!(event.pattern_name(), "test");

        let event = BulkheadEvent::CallRejected {
            pattern_name: "test".to_string(),
            timestamp: Instant::now(),
            max_concurrent_calls: 10,
        };
        assert_eq!(event.event_type(), "call_rejected");

        let event = BulkheadEvent::CallFinished {
            pattern_name: "test".to_string(),
            timestamp: Instant::now(),
            duration: Duration::from_millis(50),
        };
        assert_eq!(event.event_type(), "call_finished");

        let event = BulkheadEvent::CallFailed {
            pattern_name: "test".to_string(),
            timestamp: Instant::now(),
            duration: Duration::from_millis(50),
        };
        assert_eq!(event.event_type(), "call_failed");
    }
}
