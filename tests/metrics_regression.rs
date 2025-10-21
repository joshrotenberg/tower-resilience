//! Metrics regression tests for all resilience patterns.
//!
//! These tests ensure that metric names, types, and labels remain stable
//! across releases. Breaking changes to metrics can break user dashboards
//! and alerts, so we treat them as part of the public API.

#[cfg(feature = "metrics")]
mod metrics_regression {
    mod bulkhead;
    mod cache;
    mod chaos;
    mod circuitbreaker;
    mod core;
    mod ratelimiter;
    mod retry;
    mod timelimiter;

    /// Helper module with shared utilities for metrics testing
    pub(crate) mod helpers {
        use metrics_util::debugging::{DebugValue, DebuggingRecorder};
        use std::sync::LazyLock;

        /// Global metrics recorder for testing
        pub(crate) static RECORDER: LazyLock<DebuggingRecorder> =
            LazyLock::new(DebuggingRecorder::default);

        /// Initialize the global metrics recorder (call once per test)
        pub(crate) fn init_recorder() {
            let _ = metrics::set_global_recorder(&*RECORDER);
        }

        /// Get a snapshot of all recorded metrics
        pub(crate) fn get_metrics_snapshot() -> Vec<(
            metrics_util::CompositeKey,
            Option<metrics::Unit>,
            Option<metrics::SharedString>,
            DebugValue,
        )> {
            RECORDER.snapshotter().snapshot().into_vec()
        }

        /// Assert that a counter with the given name exists
        pub(crate) fn assert_counter_exists(name: &str) {
            let snapshot = get_metrics_snapshot();
            let found = snapshot.iter().any(|(composite_key, _, _, value)| {
                composite_key.key().name() == name && matches!(value, DebugValue::Counter(_))
            });
            assert!(found, "Expected counter '{}' not found in metrics", name);
        }

        /// Assert that a gauge with the given name exists
        pub(crate) fn assert_gauge_exists(name: &str) {
            let snapshot = get_metrics_snapshot();
            let found = snapshot.iter().any(|(composite_key, _, _, value)| {
                composite_key.key().name() == name && matches!(value, DebugValue::Gauge(_))
            });
            assert!(found, "Expected gauge '{}' not found in metrics", name);
        }

        /// Assert that a histogram with the given name exists
        pub(crate) fn assert_histogram_exists(name: &str) {
            let snapshot = get_metrics_snapshot();
            let found = snapshot.iter().any(|(composite_key, _, _, value)| {
                composite_key.key().name() == name && matches!(value, DebugValue::Histogram(_))
            });
            assert!(found, "Expected histogram '{}' not found in metrics", name);
        }

        /// Assert that a metric has a specific label
        pub(crate) fn assert_metric_has_label(name: &str, label_key: &str, label_value: &str) {
            let snapshot = get_metrics_snapshot();
            let found = snapshot.iter().any(|(composite_key, _, _, _)| {
                let key = composite_key.key();
                if key.name() == name {
                    key.labels()
                        .any(|label| label.key() == label_key && label.value() == label_value)
                } else {
                    false
                }
            });
            assert!(
                found,
                "Expected metric '{}' with label {}='{}' not found",
                name, label_key, label_value
            );
        }
    }
}
