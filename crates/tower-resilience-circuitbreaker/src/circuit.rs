use crate::config::{CircuitBreakerConfig, SlidingWindowType};
use crate::events::CircuitBreakerEvent;
#[cfg(feature = "metrics")]
use metrics::{counter, gauge, histogram};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Represents the state of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// The circuit is closed and calls are allowed.
    Closed = 0,
    /// The circuit is open and calls are rejected.
    Open = 1,
    /// The circuit is half-open and a limited number of calls are allowed.
    HalfOpen = 2,
}

/// Snapshot of circuit breaker metrics for observability.
///
/// This struct provides a point-in-time view of the circuit breaker's internal state
/// without requiring async access. All fields represent a consistent snapshot taken
/// when the metrics were retrieved.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitMetrics {
    /// Current state of the circuit breaker.
    pub state: CircuitState,
    /// Total number of recorded calls in the sliding window.
    pub total_calls: usize,
    /// Number of failed calls in the sliding window.
    pub failure_count: usize,
    /// Number of successful calls in the sliding window.
    pub success_count: usize,
    /// Number of slow calls in the sliding window.
    pub slow_call_count: usize,
    /// Current failure rate (0.0 to 1.0).
    pub failure_rate: f64,
    /// Current slow call rate (0.0 to 1.0).
    pub slow_call_rate: f64,
    /// Time since the last state transition.
    pub time_since_state_change: std::time::Duration,
}

impl CircuitState {
    pub(crate) fn from_u8(value: u8) -> Self {
        match value {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed, // Default to Closed for safety
        }
    }
}

/// Represents a call record in the time-based sliding window.
#[derive(Debug, Clone)]
struct CallRecord {
    timestamp: Instant,
    is_failure: bool,
    is_slow: bool,
}

pub(crate) struct Circuit {
    state: CircuitState,
    state_atomic: std::sync::Arc<AtomicU8>,
    last_state_change: std::time::Instant,
    // Count-based window tracking
    failure_count: usize,
    success_count: usize,
    total_count: usize,
    slow_call_count: usize,
    // Time-based window tracking
    call_records: VecDeque<CallRecord>,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new_with_atomic(std::sync::Arc::new(AtomicU8::new(
            CircuitState::Closed as u8,
        )))
    }
}

impl Circuit {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn new_with_atomic(state_atomic: std::sync::Arc<AtomicU8>) -> Self {
        Self {
            state: CircuitState::Closed,
            state_atomic,
            last_state_change: std::time::Instant::now(),
            failure_count: 0,
            success_count: 0,
            total_count: 0,
            slow_call_count: 0,
            call_records: VecDeque::new(),
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Returns a snapshot of the current circuit breaker metrics.
    ///
    /// This method provides a consistent view of all metrics at a point in time.
    /// For time-based windows, it includes all records within the current window.
    pub fn metrics(&self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) -> CircuitMetrics {
        let (total_calls, failure_count, success_count, slow_call_count) =
            match config.sliding_window_type {
                SlidingWindowType::CountBased => (
                    self.total_count,
                    self.failure_count,
                    self.success_count,
                    self.slow_call_count,
                ),
                SlidingWindowType::TimeBased => self.time_based_stats(),
            };

        let failure_rate = if total_calls > 0 {
            failure_count as f64 / total_calls as f64
        } else {
            0.0
        };

        let slow_call_rate = if total_calls > 0 {
            slow_call_count as f64 / total_calls as f64
        } else {
            0.0
        };

        CircuitMetrics {
            state: self.state,
            total_calls,
            failure_count,
            success_count,
            slow_call_count,
            failure_rate,
            slow_call_rate,
            time_since_state_change: self.last_state_change.elapsed(),
        }
    }

    /// Clean up old records from the time-based window.
    fn cleanup_old_records(&mut self, window_duration: Duration) {
        let now = Instant::now();
        while let Some(record) = self.call_records.front() {
            if now.duration_since(record.timestamp) > window_duration {
                self.call_records.pop_front();
            } else {
                break;
            }
        }
    }

    /// Calculate statistics from time-based window.
    fn time_based_stats(&self) -> (usize, usize, usize, usize) {
        let mut total = 0;
        let mut failures = 0;
        let mut successes = 0;
        let mut slow = 0;

        for record in &self.call_records {
            total += 1;
            if record.is_failure {
                failures += 1;
            } else {
                successes += 1;
            }
            if record.is_slow {
                slow += 1;
            }
        }

        (total, failures, successes, slow)
    }

    pub fn record_success(
        &mut self,
        config: &CircuitBreakerConfig<impl Sized, impl Sized>,
        duration: std::time::Duration,
    ) {
        let is_slow = config
            .slow_call_duration_threshold
            .map(|threshold| duration >= threshold)
            .unwrap_or(false);

        // Update statistics based on window type
        match config.sliding_window_type {
            SlidingWindowType::CountBased => {
                self.success_count += 1;
                self.total_count += 1;
                if is_slow {
                    self.slow_call_count += 1;
                }
            }
            SlidingWindowType::TimeBased => {
                if let Some(window_duration) = config.sliding_window_duration {
                    self.cleanup_old_records(window_duration);
                    self.call_records.push_back(CallRecord {
                        timestamp: Instant::now(),
                        is_failure: false,
                        is_slow,
                    });
                }
            }
        }

        // Emit slow call event if needed
        if is_slow {
            config
                .event_listeners
                .emit(&CircuitBreakerEvent::SlowCallDetected {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                    duration,
                    state: self.state,
                });

            #[cfg(feature = "metrics")]
            counter!("circuitbreaker_slow_calls_total", "circuitbreaker" => config.name.clone())
                .increment(1);
        }

        // Emit success event
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::SuccessRecorded {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                state: self.state,
            });

        #[cfg(feature = "metrics")]
        {
            counter!("circuitbreaker_calls_total", "circuitbreaker" => config.name.clone(), "outcome" => "success").increment(1);
            histogram!("circuitbreaker_call_duration_seconds", "circuitbreaker" => config.name.clone())
                .record(duration.as_secs_f64());
        }

        match self.state {
            CircuitState::HalfOpen => {
                let success_count = match config.sliding_window_type {
                    SlidingWindowType::CountBased => self.success_count,
                    SlidingWindowType::TimeBased => self.time_based_stats().2,
                };
                if success_count >= config.permitted_calls_in_half_open {
                    self.transition_to(CircuitState::Closed, config);
                }
            }
            _ => {
                self.evaluate_window(config);
            }
        }
    }

    pub fn record_failure(
        &mut self,
        config: &CircuitBreakerConfig<impl Sized, impl Sized>,
        duration: std::time::Duration,
    ) {
        let is_slow = config
            .slow_call_duration_threshold
            .map(|threshold| duration >= threshold)
            .unwrap_or(false);

        // Update statistics based on window type
        match config.sliding_window_type {
            SlidingWindowType::CountBased => {
                self.failure_count += 1;
                self.total_count += 1;
                if is_slow {
                    self.slow_call_count += 1;
                }
            }
            SlidingWindowType::TimeBased => {
                if let Some(window_duration) = config.sliding_window_duration {
                    self.cleanup_old_records(window_duration);
                    self.call_records.push_back(CallRecord {
                        timestamp: Instant::now(),
                        is_failure: true,
                        is_slow,
                    });
                }
            }
        }

        // Emit slow call event if needed
        if is_slow {
            config
                .event_listeners
                .emit(&CircuitBreakerEvent::SlowCallDetected {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                    duration,
                    state: self.state,
                });

            #[cfg(feature = "metrics")]
            counter!("circuitbreaker_slow_calls_total", "circuitbreaker" => config.name.clone())
                .increment(1);
        }

        // Emit failure event
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::FailureRecorded {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                state: self.state,
            });

        #[cfg(feature = "metrics")]
        {
            counter!("circuitbreaker_calls_total", "circuitbreaker" => config.name.clone(), "outcome" => "failure").increment(1);
            histogram!("circuitbreaker_call_duration_seconds", "circuitbreaker" => config.name.clone())
                .record(duration.as_secs_f64());
        }

        match self.state {
            CircuitState::HalfOpen => {
                self.transition_to(CircuitState::Open, config);
            }
            _ => {
                self.evaluate_window(config);
            }
        }
    }

    pub fn try_acquire(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) -> bool {
        match self.state {
            CircuitState::Closed => {
                config
                    .event_listeners
                    .emit(&CircuitBreakerEvent::CallPermitted {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        state: self.state,
                    });
                true
            }
            CircuitState::Open => {
                if self.last_state_change.elapsed() >= config.wait_duration_in_open {
                    self.transition_to(CircuitState::HalfOpen, config);
                    config
                        .event_listeners
                        .emit(&CircuitBreakerEvent::CallPermitted {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            state: self.state,
                        });
                    true
                } else {
                    config
                        .event_listeners
                        .emit(&CircuitBreakerEvent::CallRejected {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                        });
                    false
                }
            }
            CircuitState::HalfOpen => {
                let permitted =
                    self.success_count + self.failure_count < config.permitted_calls_in_half_open;
                if permitted {
                    config
                        .event_listeners
                        .emit(&CircuitBreakerEvent::CallPermitted {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                            state: self.state,
                        });
                } else {
                    config
                        .event_listeners
                        .emit(&CircuitBreakerEvent::CallRejected {
                            pattern_name: config.name.clone(),
                            timestamp: Instant::now(),
                        });
                }
                permitted
            }
        }
    }

    pub fn force_open(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        self.transition_to(CircuitState::Open, config);
    }

    pub fn force_closed(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        self.transition_to(CircuitState::Closed, config);
    }

    pub fn reset(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        self.transition_to(CircuitState::Closed, config);
    }

    fn transition_to(
        &mut self,
        state: CircuitState,
        config: &CircuitBreakerConfig<impl Sized, impl Sized>,
    ) {
        if self.state == state {
            return;
        }

        let from_state = self.state;

        // Emit event
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::StateTransition {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                from_state,
                to_state: state,
            });

        #[cfg(feature = "tracing")]
        tracing::info!(from = ?from_state, to = ?state, "Circuit state transition");

        #[cfg(feature = "metrics")]
        {
            counter!(
                "circuitbreaker_transitions_total",
                "circuitbreaker" => config.name.clone(),
                "from" => match from_state {
                    CircuitState::Closed => "Closed",
                    CircuitState::Open => "Open",
                    CircuitState::HalfOpen => "HalfOpen",
                },
                "to" => match state {
                    CircuitState::Closed => "Closed",
                    CircuitState::Open => "Open",
                    CircuitState::HalfOpen => "HalfOpen",
                }
            )
            .increment(1);

            gauge!("circuitbreaker_state", "circuitbreaker" => config.name.clone(), "state" => match state {
                CircuitState::Closed => "Closed",
                CircuitState::Open => "Open",
                CircuitState::HalfOpen => "HalfOpen",
            })
            .set(1.0);
        }

        self.state = state;
        self.state_atomic.store(state as u8, Ordering::Release);
        self.last_state_change = std::time::Instant::now();
        self.success_count = 0;
        self.failure_count = 0;
        self.total_count = 0;
        self.slow_call_count = 0;
        self.call_records.clear();
    }

    fn evaluate_window(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        let (total_count, failure_count, _success_count, slow_call_count) =
            match config.sliding_window_type {
                SlidingWindowType::CountBased => (
                    self.total_count,
                    self.failure_count,
                    self.success_count,
                    self.slow_call_count,
                ),
                SlidingWindowType::TimeBased => {
                    if let Some(window_duration) = config.sliding_window_duration {
                        self.cleanup_old_records(window_duration);
                    }
                    self.time_based_stats()
                }
            };

        // Don't evaluate until minimum calls threshold is met
        if total_count < config.minimum_number_of_calls {
            return;
        }

        // For count-based window, also check if window is full
        if config.sliding_window_type == SlidingWindowType::CountBased
            && total_count < config.sliding_window_size
        {
            return;
        }

        let failure_rate = failure_count as f64 / total_count as f64;
        let slow_call_rate = slow_call_count as f64 / total_count as f64;

        // Open if either failure rate or slow call rate exceeds threshold
        let should_open = failure_rate >= config.failure_rate_threshold
            || (config.slow_call_duration_threshold.is_some()
                && slow_call_rate >= config.slow_call_rate_threshold);

        if should_open {
            self.transition_to(CircuitState::Open, config);
        }
        // Don't transition to closed if we're in HalfOpen - that happens via record_success
    }
}
