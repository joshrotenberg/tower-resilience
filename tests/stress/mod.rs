//! Stress tests for tower-resilience patterns
//!
//! These tests push the patterns to their limits to validate behavior under extreme conditions.
//! They are marked with `#[ignore]` and must be run explicitly:
//!
//! ```bash
//! # Run all stress tests
//! cargo test --test stress -- --ignored
//!
//! # Run specific stress test module
//! cargo test --test stress circuitbreaker -- --ignored
//!
//! # Run with output
//! cargo test --test stress -- --ignored --nocapture
//! ```
//!
//! ## What We Test
//!
//! - **High volume**: Millions of operations
//! - **High concurrency**: Thousands of concurrent requests
//! - **Memory usage**: Large data structures, leak detection
//! - **State consistency**: Correctness under stress
//! - **Resource cleanup**: No panics, deadlocks, or leaks
//! - **Performance degradation**: Acceptable behavior under load

pub mod bulkhead;
pub mod cache;
pub mod circuitbreaker;
pub mod composition;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Utility: Track peak concurrent operations
pub struct ConcurrencyTracker {
    current: AtomicUsize,
    peak: AtomicUsize,
}

impl ConcurrencyTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }

    pub fn enter(&self) {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(current, Ordering::SeqCst);
    }

    pub fn exit(&self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    pub fn current(&self) -> usize {
        self.current.load(Ordering::SeqCst)
    }
}

/// Utility: Memory usage statistics
#[cfg(target_os = "macos")]
pub fn get_memory_usage_mb() -> f64 {
    use std::process::Command;
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("failed to get memory usage");

    let rss_kb = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0);

    rss_kb / 1024.0 // Convert KB to MB
}

#[cfg(not(target_os = "macos"))]
pub fn get_memory_usage_mb() -> f64 {
    // Platform-agnostic fallback - just return 0
    0.0
}

/// Utility: Generate load pattern
pub enum LoadPattern {
    Constant(usize),
    Burst {
        requests: usize,
        bursts: usize,
    },
    Ramp {
        start: usize,
        end: usize,
        steps: usize,
    },
}

impl LoadPattern {
    pub fn total_requests(&self) -> usize {
        match self {
            LoadPattern::Constant(n) => *n,
            LoadPattern::Burst { requests, bursts } => requests * bursts,
            LoadPattern::Ramp { start, end, steps } => {
                (0..*steps).map(|i| start + (end - start) * i / steps).sum()
            }
        }
    }
}
