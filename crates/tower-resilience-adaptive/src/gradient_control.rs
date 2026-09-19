//! Netflix Gradient2-inspired adaptive concurrency control.
//!
//! The design follows the Gradient2 approach described by [Netflix's
//! `concurrency-limits` project](https://github.com/Netflix/concurrency-limits)
//! and was cross-checked against [Tower's `tower-acc`
//! implementation](https://github.com/guilload/tower-acc). This is an
//! independent implementation using this crate's validated builders and Tower
//! service lifecycle.

use std::sync::Mutex;
use std::time::Duration;

use super::ConcurrencyAlgorithm;

/// Gradient2 adaptive concurrency algorithm.
///
/// Gradient2 compares the latest RTT with a slowly changing RTT baseline. A
/// rising latest RTT indicates queueing and reduces the limit; healthy RTT at
/// sufficient utilization allows the limit to grow. Load is deliberately part
/// of the update signal: a quiet service does not increase its limit merely
/// because requests happen to be fast.
pub struct Gradient2 {
    state: Mutex<State>,
    min_limit: usize,
    max_limit: usize,
    smoothing: f64,
    rtt_tolerance: f64,
    long_window: usize,
    warmup_samples: usize,
}

#[derive(Debug)]
struct State {
    limit: f64,
    latest_rtt_nanos: Option<f64>,
    long_rtt_nanos: Option<f64>,
    warmup_sum: f64,
    samples: usize,
}

impl Gradient2 {
    /// Create a builder for Gradient2.
    pub fn builder() -> Gradient2Builder {
        Gradient2Builder::default()
    }

    fn update(&self, latency: Duration, in_flight: usize) {
        let rtt = latency.as_nanos() as f64;
        if rtt <= 0.0 {
            return;
        }

        let mut state = self.state.lock().expect("gradient2 state lock poisoned");
        state.latest_rtt_nanos = Some(rtt);
        state.samples = state.samples.saturating_add(1);
        state.warmup_sum += rtt;
        state.long_rtt_nanos = Some(if state.samples <= self.warmup_samples {
            state.warmup_sum / state.samples as f64
        } else {
            let baseline = state.long_rtt_nanos.unwrap_or(rtt);
            baseline * (1.0 - self.long_factor()) + rtt * self.long_factor()
        });

        // A completion observes itself as in flight. This is intentional: it
        // describes the load under which the RTT was measured.
        if in_flight.saturating_mul(2) < state.limit.ceil() as usize {
            return;
        }

        let baseline = state.long_rtt_nanos.expect("baseline initialized above");
        let latest = state
            .latest_rtt_nanos
            .expect("latest RTT initialized above");
        let gradient = (self.rtt_tolerance * baseline / latest).clamp(0.5, 1.0);
        let queue_allowance = log10_queue_size(state.limit.ceil() as usize);
        let target = gradient * state.limit + queue_allowance as f64;
        state.limit = ((1.0 - self.smoothing) * state.limit + self.smoothing * target)
            .clamp(self.min_limit as f64, self.max_limit as f64);
    }

    fn long_factor(&self) -> f64 {
        2.0 / (self.long_window as f64 + 1.0)
    }
}

impl ConcurrencyAlgorithm for Gradient2 {
    fn record_success(&self, latency: Duration) {
        self.update(latency, self.limit());
    }

    fn record_success_with_load(&self, latency: Duration, in_flight: usize) {
        self.update(latency, in_flight);
    }

    fn record_failure(&self) {
        let mut state = self.state.lock().expect("gradient2 state lock poisoned");
        state.limit = (state.limit / 2.0).max(self.min_limit as f64);
    }

    fn record_dropped(&self) {}

    fn limit(&self) -> usize {
        let state = self.state.lock().expect("gradient2 state lock poisoned");
        (state.limit.round() as usize).clamp(self.min_limit, self.max_limit)
    }

    fn min_limit(&self) -> usize {
        self.min_limit
    }

    fn max_limit(&self) -> usize {
        self.max_limit
    }
}

fn log10_queue_size(limit: usize) -> usize {
    let limit = limit.max(1);
    let digits = limit.ilog10() as usize;
    digits
        + if 10usize.pow(digits as u32) == limit {
            0
        } else {
            1
        }
}

/// Builder for [`Gradient2`].
///
/// Defaults are an initial limit of `20`, bounds of `1..=200`, smoothing `0.2`,
/// RTT tolerance `1.5`, a `600`-sample long-term window, and `10` warmup
/// samples.
#[derive(Debug, Clone)]
pub struct Gradient2Builder {
    initial_limit: usize,
    min_limit: usize,
    max_limit: usize,
    smoothing: f64,
    rtt_tolerance: f64,
    long_window: usize,
    warmup_samples: usize,
}

impl Default for Gradient2Builder {
    fn default() -> Self {
        Self {
            initial_limit: 20,
            min_limit: 1,
            max_limit: 200,
            smoothing: 0.2,
            rtt_tolerance: 1.5,
            long_window: 600,
            warmup_samples: 10,
        }
    }
}

impl Gradient2Builder {
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
    /// Set the smoothing factor for limit updates, in `(0, 1]`.
    pub fn smoothing(mut self, value: f64) -> Self {
        self.smoothing = value;
        self
    }
    /// Set the tolerated latest-to-baseline RTT increase, at least `1.0`.
    pub fn rtt_tolerance(mut self, value: f64) -> Self {
        self.rtt_tolerance = value;
        self
    }
    /// Set the long-term RTT window in samples.
    pub fn long_window(mut self, value: usize) -> Self {
        self.long_window = value;
        self
    }
    /// Set the number of samples used to initialize the RTT baseline.
    pub fn warmup_samples(mut self, value: usize) -> Self {
        self.warmup_samples = value;
        self
    }

    /// Build a validated Gradient2 algorithm.
    pub fn build(self) -> Result<Gradient2, Gradient2ConfigError> {
        if self.min_limit > self.max_limit {
            return Err(Gradient2ConfigError::MinExceedsMax {
                min_limit: self.min_limit,
                max_limit: self.max_limit,
            });
        }
        if !self.smoothing.is_finite()
            || !(0.0..=1.0).contains(&self.smoothing)
            || self.smoothing == 0.0
        {
            return Err(Gradient2ConfigError::InvalidSmoothing(self.smoothing));
        }
        if !self.rtt_tolerance.is_finite() || self.rtt_tolerance < 1.0 {
            return Err(Gradient2ConfigError::InvalidRttTolerance(
                self.rtt_tolerance,
            ));
        }
        if self.long_window == 0 {
            return Err(Gradient2ConfigError::ZeroLongWindow);
        }
        if self.warmup_samples == 0 {
            return Err(Gradient2ConfigError::ZeroWarmupSamples);
        }
        Ok(Gradient2 {
            state: Mutex::new(State {
                limit: self.initial_limit.clamp(self.min_limit, self.max_limit) as f64,
                latest_rtt_nanos: None,
                long_rtt_nanos: None,
                warmup_sum: 0.0,
                samples: 0,
            }),
            min_limit: self.min_limit,
            max_limit: self.max_limit,
            smoothing: self.smoothing,
            rtt_tolerance: self.rtt_tolerance,
            long_window: self.long_window,
            warmup_samples: self.warmup_samples,
        })
    }
}

/// Errors returned when building [`Gradient2`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Gradient2ConfigError {
    /// The minimum limit exceeds the maximum limit.
    #[error("min_limit ({min_limit}) must not exceed max_limit ({max_limit})")]
    MinExceedsMax { min_limit: usize, max_limit: usize },
    /// The smoothing factor is not finite or is outside `(0, 1]`.
    #[error("smoothing must be finite and in (0, 1], got {0}")]
    InvalidSmoothing(f64),
    /// The RTT tolerance is not finite or is below `1.0`.
    #[error("rtt_tolerance must be finite and at least 1.0, got {0}")]
    InvalidRttTolerance(f64),
    /// The long-term RTT window cannot be empty.
    #[error("long_window must be greater than zero")]
    ZeroLongWindow,
    /// The warmup period cannot be empty.
    #[error("warmup_samples must be greater than zero")]
    ZeroWarmupSamples,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warmed() -> Gradient2 {
        let algorithm = Gradient2::builder()
            .initial_limit(20)
            .max_limit(100)
            .build()
            .unwrap();
        for _ in 0..10 {
            algorithm.record_success_with_load(Duration::from_millis(10), 20);
        }
        algorithm
    }

    #[test]
    fn grows_only_when_sufficiently_loaded() {
        let algorithm = warmed();
        let before = algorithm.limit();
        for _ in 0..20 {
            algorithm.record_success_with_load(Duration::from_millis(10), 1);
        }
        assert_eq!(algorithm.limit(), before);
        for _ in 0..20 {
            algorithm.record_success_with_load(Duration::from_millis(10), before);
        }
        assert!(algorithm.limit() > before);
    }

    #[test]
    fn reduces_after_latency_spike() {
        let algorithm = warmed();
        let before = algorithm.limit();
        for _ in 0..30 {
            algorithm.record_success_with_load(Duration::from_millis(100), before);
        }
        assert!(algorithm.limit() < before);
    }

    #[test]
    fn mixed_fast_and_slow_requests_reduce_the_limit() {
        let algorithm = warmed();
        let before = algorithm.limit();
        for latency in [10, 10, 10, 10, 80, 80, 80, 80, 80] {
            algorithm.record_success_with_load(Duration::from_millis(latency), before);
        }
        assert!(algorithm.limit() < before);
    }

    #[test]
    fn respects_bounds_and_failures() {
        let algorithm = Gradient2::builder()
            .initial_limit(5)
            .min_limit(3)
            .max_limit(5)
            .build()
            .unwrap();
        for _ in 0..100 {
            algorithm.record_success_with_load(Duration::from_millis(1), 5);
        }
        assert_eq!(algorithm.limit(), 5);
        algorithm.record_failure();
        assert_eq!(algorithm.limit(), 3);
        algorithm.record_dropped();
        assert_eq!(algorithm.limit(), 3);
    }

    #[test]
    fn rejects_invalid_configuration() {
        assert!(matches!(
            Gradient2::builder().min_limit(2).max_limit(1).build(),
            Err(Gradient2ConfigError::MinExceedsMax { .. })
        ));
        assert!(matches!(
            Gradient2::builder().smoothing(0.0).build(),
            Err(Gradient2ConfigError::InvalidSmoothing(_))
        ));
        assert!(matches!(
            Gradient2::builder().rtt_tolerance(0.9).build(),
            Err(Gradient2ConfigError::InvalidRttTolerance(_))
        ));
        assert!(matches!(
            Gradient2::builder().long_window(0).build(),
            Err(Gradient2ConfigError::ZeroLongWindow)
        ));
    }
}
