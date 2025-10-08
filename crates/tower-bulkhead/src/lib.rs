//! Bulkhead pattern for Tower services.
//!
//! The bulkhead pattern isolates resources to prevent cascading failures.
//! This implementation uses semaphore-based concurrency limiting to control
//! the maximum number of concurrent calls to a service.
//!
//! # Example
//!
//! ```rust
//! use tower::ServiceBuilder;
//! use tower_bulkhead::BulkheadConfig;
//! use std::time::Duration;
//!
//! # async fn example() {
//! let layer = BulkheadConfig::builder()
//!     .max_concurrent_calls(10)
//!     .max_wait_duration(Some(Duration::from_secs(1)))
//!     .name("my-bulkhead")
//!     .on_call_permitted(|concurrent| {
//!         println!("Call permitted, {} concurrent calls", concurrent);
//!     })
//!     .on_call_rejected(|max| {
//!         println!("Call rejected, max {} calls", max);
//!     })
//!     .build();
//!
//! let service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(|req: String| async move {
//!         Ok::<_, std::io::Error>(req)
//!     });
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn test_config_builder_defaults() {
        let config = BulkheadConfig::builder().build();
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
