//! Adaptive concurrency control algorithms.
//!
//! This module provides different algorithms for dynamically adjusting
//! concurrency limits based on observed latency and error rates.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tower_resilience_core::aimd::{AimdConfig, AimdController};

/// Trait for adaptive concurrency control algorithms.
pub trait ConcurrencyAlgorithm: Send + Sync {
    /// Record a successful request with its latency.
    fn record_success(&self, latency: Duration);

    /// Record a failed request.
    fn record_failure(&self);

    /// Record a dropped/cancelled request.
    fn record_dropped(&self);

    /// Get the current concurrency limit.
    fn limit(&self) -> usize;

    /// Get the minimum allowed limit.
    fn min_limit(&self) -> usize;

    /// Get the maximum allowed limit.
    fn max_limit(&self) -> usize;
}

/// AIMD (Additive Increase Multiplicative Decrease) algorithm.
///
/// This is the classic TCP congestion control algorithm:
/// - On success: increase limit by a fixed amount
/// - On failure/timeout: decrease limit by a factor
///
/// The algorithm creates a "sawtooth" pattern as it probes for capacity.
pub struct Aimd {
    controller: AimdController,
    /// Latency threshold above which we consider the system congested.
    latency_threshold: Duration,
}

impl Aimd {
    /// Create a new AIMD algorithm with the given configuration.
    pub fn new(config: AimdConfig, latency_threshold: Duration) -> Self {
        Self {
            controller: AimdController::new(config),
            latency_threshold,
        }
    }

    /// Create a builder for configuring AIMD.
    pub fn builder() -> AimdBuilder {
        AimdBuilder::default()
    }
}

impl ConcurrencyAlgorithm for Aimd {
    fn record_success(&self, latency: Duration) {
        if latency > self.latency_threshold {
            // High latency indicates congestion
            self.controller.record_failure();
        } else {
            self.controller.record_success();
        }
    }

    fn record_failure(&self) {
        self.controller.record_failure();
    }

    fn record_dropped(&self) {
        // Dropped requests don't affect the limit
    }

    fn limit(&self) -> usize {
        self.controller.limit()
    }

    fn min_limit(&self) -> usize {
        self.controller.min_limit()
    }

    fn max_limit(&self) -> usize {
        self.controller.max_limit()
    }
}

/// Builder for AIMD algorithm.
#[derive(Debug, Clone)]
pub struct AimdBuilder {
    initial_limit: usize,
    min_limit: usize,
    max_limit: usize,
    increase_by: usize,
    decrease_factor: f64,
    latency_threshold: Duration,
}

impl Default for AimdBuilder {
    fn default() -> Self {
        Self {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 100,
            increase_by: 1,
            decrease_factor: 0.5,
            latency_threshold: Duration::from_millis(100),
        }
    }
}

impl AimdBuilder {
    /// Set the initial concurrency limit.
    pub fn initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    /// Set the minimum concurrency limit.
    pub fn min_limit(mut self, limit: usize) -> Self {
        self.min_limit = limit;
        self
    }

    /// Set the maximum concurrency limit.
    pub fn max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    /// Set the additive increase amount per success.
    pub fn increase_by(mut self, amount: usize) -> Self {
        self.increase_by = amount;
        self
    }

    /// Set the multiplicative decrease factor on failure.
    pub fn decrease_factor(mut self, factor: f64) -> Self {
        self.decrease_factor = factor;
        self
    }

    /// Set the latency threshold for congestion detection.
    ///
    /// Requests taking longer than this are considered a congestion signal.
    pub fn latency_threshold(mut self, threshold: Duration) -> Self {
        self.latency_threshold = threshold;
        self
    }

    /// Build the AIMD algorithm.
    pub fn build(self) -> Aimd {
        let config = AimdConfig::new()
            .with_initial_limit(self.initial_limit)
            .with_min_limit(self.min_limit)
            .with_max_limit(self.max_limit)
            .with_increase_by(self.increase_by)
            .with_decrease_factor(self.decrease_factor);

        Aimd::new(config, self.latency_threshold)
    }
}

/// TCP Vegas algorithm for concurrency control.
///
/// Vegas uses RTT (round-trip time) measurements to detect congestion
/// before it causes packet loss. It estimates the queue depth and adjusts
/// the concurrency limit to maintain a target queue size.
///
/// This is more stable than AIMD and avoids the sawtooth pattern.
pub struct Vegas {
    /// Current limit
    limit: AtomicUsize,
    /// Minimum limit
    min_limit: usize,
    /// Maximum limit
    max_limit: usize,
    /// Minimum observed RTT (used as baseline)
    min_rtt_nanos: AtomicU64,
    /// Alpha threshold - if queue estimate < alpha, increase
    alpha: usize,
    /// Beta threshold - if queue estimate > beta, decrease
    beta: usize,
    /// Smoothing factor for RTT measurements
    smoothing: f64,
    /// Smoothed RTT in nanoseconds
    smoothed_rtt_nanos: AtomicU64,
    /// Number of samples collected
    sample_count: AtomicUsize,
    /// Minimum samples before adjusting
    min_samples: usize,
}

impl Vegas {
    /// Create a new Vegas algorithm.
    pub fn new(
        initial_limit: usize,
        min_limit: usize,
        max_limit: usize,
        alpha: usize,
        beta: usize,
    ) -> Self {
        Self {
            limit: AtomicUsize::new(initial_limit.clamp(min_limit, max_limit)),
            min_limit,
            max_limit,
            min_rtt_nanos: AtomicU64::new(u64::MAX),
            alpha,
            beta,
            smoothing: 0.5,
            smoothed_rtt_nanos: AtomicU64::new(0),
            sample_count: AtomicUsize::new(0),
            min_samples: 10,
        }
    }

    /// Create a builder for Vegas.
    pub fn builder() -> VegasBuilder {
        VegasBuilder::default()
    }

    fn update_rtt(&self, rtt: Duration) {
        let rtt_nanos = rtt.as_nanos() as u64;

        // Update minimum RTT
        let mut current_min = self.min_rtt_nanos.load(Ordering::Relaxed);
        while rtt_nanos < current_min {
            match self.min_rtt_nanos.compare_exchange_weak(
                current_min,
                rtt_nanos,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current_min = c,
            }
        }

        // Update smoothed RTT using exponential moving average
        let current_smoothed = self.smoothed_rtt_nanos.load(Ordering::Relaxed);
        let new_smoothed = if current_smoothed == 0 {
            rtt_nanos
        } else {
            (self.smoothing * rtt_nanos as f64 + (1.0 - self.smoothing) * current_smoothed as f64)
                as u64
        };
        self.smoothed_rtt_nanos
            .store(new_smoothed, Ordering::Relaxed);

        self.sample_count.fetch_add(1, Ordering::Relaxed);
    }

    fn adjust_limit(&self) {
        // Don't adjust until we have enough samples
        if self.sample_count.load(Ordering::Relaxed) < self.min_samples {
            return;
        }

        let min_rtt = self.min_rtt_nanos.load(Ordering::Relaxed);
        let smoothed_rtt = self.smoothed_rtt_nanos.load(Ordering::Relaxed);

        if min_rtt == u64::MAX || min_rtt == 0 || smoothed_rtt == 0 {
            return;
        }

        let current_limit = self.limit.load(Ordering::Relaxed);

        // Estimate queue depth: (smoothed_rtt - min_rtt) / min_rtt * current_limit
        // This estimates how many requests are "queued" beyond the minimum RTT
        let queue_estimate = if smoothed_rtt > min_rtt {
            ((smoothed_rtt - min_rtt) as f64 / min_rtt as f64 * current_limit as f64) as usize
        } else {
            0
        };

        let new_limit = if queue_estimate < self.alpha {
            // Under-utilized, increase
            (current_limit + 1).min(self.max_limit)
        } else if queue_estimate > self.beta {
            // Congested, decrease
            (current_limit.saturating_sub(1)).max(self.min_limit)
        } else {
            // In the sweet spot
            current_limit
        };

        self.limit.store(new_limit, Ordering::Relaxed);
    }
}

impl ConcurrencyAlgorithm for Vegas {
    fn record_success(&self, latency: Duration) {
        self.update_rtt(latency);
        self.adjust_limit();
    }

    fn record_failure(&self) {
        // On error, decrease limit immediately
        let current = self.limit.load(Ordering::Relaxed);
        let new_limit = (current / 2).max(self.min_limit);
        self.limit.store(new_limit, Ordering::Relaxed);
    }

    fn record_dropped(&self) {
        // Dropped requests don't affect the limit
    }

    fn limit(&self) -> usize {
        self.limit.load(Ordering::Relaxed)
    }

    fn min_limit(&self) -> usize {
        self.min_limit
    }

    fn max_limit(&self) -> usize {
        self.max_limit
    }
}

/// Builder for Vegas algorithm.
#[derive(Debug, Clone)]
pub struct VegasBuilder {
    initial_limit: usize,
    min_limit: usize,
    max_limit: usize,
    alpha: usize,
    beta: usize,
}

impl Default for VegasBuilder {
    fn default() -> Self {
        Self {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 100,
            alpha: 3,
            beta: 6,
        }
    }
}

impl VegasBuilder {
    /// Set the initial concurrency limit.
    pub fn initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    /// Set the minimum concurrency limit.
    pub fn min_limit(mut self, limit: usize) -> Self {
        self.min_limit = limit;
        self
    }

    /// Set the maximum concurrency limit.
    pub fn max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    /// Set the alpha threshold (queue depth for increase).
    ///
    /// When the estimated queue depth is below alpha, the limit increases.
    pub fn alpha(mut self, alpha: usize) -> Self {
        self.alpha = alpha;
        self
    }

    /// Set the beta threshold (queue depth for decrease).
    ///
    /// When the estimated queue depth is above beta, the limit decreases.
    pub fn beta(mut self, beta: usize) -> Self {
        self.beta = beta;
        self
    }

    /// Build the Vegas algorithm.
    pub fn build(self) -> Vegas {
        Vegas::new(
            self.initial_limit,
            self.min_limit,
            self.max_limit,
            self.alpha,
            self.beta,
        )
    }
}

/// Algorithm selection enum for the adaptive limiter.
pub enum Algorithm {
    /// AIMD algorithm
    Aimd(Aimd),
    /// Vegas algorithm
    Vegas(Vegas),
}

impl ConcurrencyAlgorithm for Algorithm {
    fn record_success(&self, latency: Duration) {
        match self {
            Algorithm::Aimd(a) => a.record_success(latency),
            Algorithm::Vegas(v) => v.record_success(latency),
        }
    }

    fn record_failure(&self) {
        match self {
            Algorithm::Aimd(a) => a.record_failure(),
            Algorithm::Vegas(v) => v.record_failure(),
        }
    }

    fn record_dropped(&self) {
        match self {
            Algorithm::Aimd(a) => a.record_dropped(),
            Algorithm::Vegas(v) => v.record_dropped(),
        }
    }

    fn limit(&self) -> usize {
        match self {
            Algorithm::Aimd(a) => a.limit(),
            Algorithm::Vegas(v) => v.limit(),
        }
    }

    fn min_limit(&self) -> usize {
        match self {
            Algorithm::Aimd(a) => a.min_limit(),
            Algorithm::Vegas(v) => v.min_limit(),
        }
    }

    fn max_limit(&self) -> usize {
        match self {
            Algorithm::Aimd(a) => a.max_limit(),
            Algorithm::Vegas(v) => v.max_limit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aimd_builder() {
        let aimd = Aimd::builder()
            .initial_limit(20)
            .min_limit(5)
            .max_limit(200)
            .increase_by(2)
            .decrease_factor(0.75)
            .latency_threshold(Duration::from_millis(50))
            .build();

        assert_eq!(aimd.limit(), 20);
        assert_eq!(aimd.min_limit(), 5);
        assert_eq!(aimd.max_limit(), 200);
    }

    #[test]
    fn test_aimd_success_increases() {
        let aimd = Aimd::builder()
            .initial_limit(10)
            .increase_by(1)
            .latency_threshold(Duration::from_millis(100))
            .build();

        // Fast request - should increase
        aimd.record_success(Duration::from_millis(50));
        assert_eq!(aimd.limit(), 11);
    }

    #[test]
    fn test_aimd_high_latency_decreases() {
        let aimd = Aimd::builder()
            .initial_limit(10)
            .decrease_factor(0.5)
            .latency_threshold(Duration::from_millis(100))
            .build();

        // Slow request - should decrease
        aimd.record_success(Duration::from_millis(150));
        assert_eq!(aimd.limit(), 5);
    }

    #[test]
    fn test_aimd_failure_decreases() {
        let aimd = Aimd::builder()
            .initial_limit(10)
            .decrease_factor(0.5)
            .build();

        aimd.record_failure();
        assert_eq!(aimd.limit(), 5);
    }

    #[test]
    fn test_vegas_builder() {
        let vegas = Vegas::builder()
            .initial_limit(20)
            .min_limit(5)
            .max_limit(200)
            .alpha(2)
            .beta(8)
            .build();

        assert_eq!(vegas.limit(), 20);
        assert_eq!(vegas.min_limit(), 5);
        assert_eq!(vegas.max_limit(), 200);
    }

    #[test]
    fn test_vegas_failure_decreases() {
        let vegas = Vegas::builder().initial_limit(20).min_limit(1).build();

        vegas.record_failure();
        assert_eq!(vegas.limit(), 10);
    }

    #[test]
    fn test_vegas_min_rtt_tracking() {
        let vegas = Vegas::builder().initial_limit(10).build();

        vegas.record_success(Duration::from_millis(100));
        vegas.record_success(Duration::from_millis(50));
        vegas.record_success(Duration::from_millis(75));

        // Min RTT should be 50ms
        let min_rtt = vegas.min_rtt_nanos.load(Ordering::Relaxed);
        assert_eq!(min_rtt, Duration::from_millis(50).as_nanos() as u64);
    }

    #[test]
    fn test_algorithm_enum() {
        let aimd = Algorithm::Aimd(Aimd::builder().initial_limit(10).build());
        assert_eq!(aimd.limit(), 10);

        let vegas = Algorithm::Vegas(Vegas::builder().initial_limit(20).build());
        assert_eq!(vegas.limit(), 20);
    }
}
