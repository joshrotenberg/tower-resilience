use crate::config::{CircuitBreakerConfig, FailureModel, SlidingWindowType};
use crate::events::CircuitBreakerEvent;
#[cfg(feature = "metrics")]
use metrics::{counter, gauge, histogram};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Represents the state of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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

/// Outcome of an admission attempt made by [`Circuit::try_acquire`].
pub(crate) enum Admission {
    /// The call is rejected. No probe reservation was made.
    Rejected,
    /// The call is admitted.
    ///
    /// `Some(permit)` is a `HalfOpen` admission: the caller must call
    /// `.forget()` on the permit once the call's result has been recorded
    /// (permanently consuming the reservation for this half-open cycle), or
    /// simply drop it if the call is cancelled before completing (returning
    /// the reservation to the pool without recording a result). `None` is a
    /// `Closed` admission, which does not consume half-open capacity.
    Admitted(Option<OwnedSemaphorePermit>),
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
    // FailureModel::ConsecutiveFailures tracking. Resets to 0 on every
    // success and on every state transition to Closed/HalfOpen.
    consecutive_failures: usize,
    // Half-open probe admission for the *current* half-open cycle. Reset to
    // a fresh `Semaphore` (a new `Arc`, not a resized existing one) every
    // time the circuit transitions into `HalfOpen`. A fresh `Arc` per cycle
    // means a permit released late by a probe admitted under a previous
    // cycle (e.g. cancelled well after the circuit moved on) drops into the
    // old, orphaned semaphore instance and can never leak capacity into the
    // current cycle.
    half_open_permits: Arc<Semaphore>,
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
            consecutive_failures: 0,
            // No half-open cycle is active yet; `transition_to` allocates
            // the real semaphore the first time the circuit enters
            // `HalfOpen`.
            half_open_permits: Arc::new(Semaphore::new(0)),
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Returns a snapshot of the current circuit breaker metrics.
    ///
    /// This method provides a consistent view of all metrics at a point in time.
    /// For time-based windows, it includes all records within the current window.
    pub fn metrics<C>(&self, config: &CircuitBreakerConfig<C>) -> CircuitMetrics {
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

    pub fn record_success<C>(
        &mut self,
        config: &CircuitBreakerConfig<C>,
        duration: std::time::Duration,
    ) {
        let is_slow = config
            .slow_call_duration_threshold
            .map(|threshold| duration >= threshold)
            .unwrap_or(false);

        // A success resets the consecutive-failure counter regardless of the
        // configured FailureModel; see ConsecutiveFailures.
        self.consecutive_failures = 0;

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

        // Manual/external-only mode: the counters, events, and metrics above
        // are still recorded for observability, but the circuit never trips
        // or recovers based on the outcome. State only changes via an
        // explicit force_open/force_closed/reset call.
        if !config.manual_mode {
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
    }

    pub fn record_failure<C>(
        &mut self,
        config: &CircuitBreakerConfig<C>,
        duration: std::time::Duration,
    ) {
        let is_slow = config
            .slow_call_duration_threshold
            .map(|threshold| duration >= threshold)
            .unwrap_or(false);

        // Track consecutive failures for FailureModel::ConsecutiveFailures.
        // Maintained regardless of the active model so switching models via
        // observability or live config is consistent.
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);

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

        // See the identical guard in `record_success` -- manual/external-only
        // mode never trips or recovers based on inner-service results.
        if !config.manual_mode {
            match self.state {
                CircuitState::HalfOpen => {
                    self.transition_to(CircuitState::Open, config);
                }
                _ => {
                    self.evaluate_window(config);
                }
            }
        }
    }

    pub fn try_acquire<C>(&mut self, config: &CircuitBreakerConfig<C>) -> Admission {
        match self.state {
            CircuitState::Closed => {
                config
                    .event_listeners
                    .emit(&CircuitBreakerEvent::CallPermitted {
                        pattern_name: config.name.clone(),
                        timestamp: Instant::now(),
                        state: self.state,
                    });
                Admission::Admitted(None)
            }
            CircuitState::Open => {
                // In manual mode the `wait_duration_in_open` recovery timer
                // is disabled: the circuit stays open (rejecting) until
                // something explicitly closes or resets it.
                if !config.manual_mode
                    && self.last_state_change.elapsed() >= config.wait_duration_in_open
                {
                    self.transition_to(CircuitState::HalfOpen, config);
                    // `transition_to` just allocated a fresh half-open
                    // semaphore; evaluate admission again against the new
                    // state so the very first probe of the cycle goes
                    // through the same reservation path as every other one.
                    self.try_acquire(config)
                } else {
                    self.record_rejection(config);
                    Admission::Rejected
                }
            }
            CircuitState::HalfOpen => {
                // Reserve a slot atomically. Unlike the old
                // `success_count + failure_count < permitted` check, this
                // counts admissions the instant they are granted, not the
                // instant they complete -- so concurrent clones racing this
                // call cannot all observe spare capacity before any of them
                // finishes.
                match Arc::clone(&self.half_open_permits).try_acquire_owned() {
                    Ok(permit) => {
                        config
                            .event_listeners
                            .emit(&CircuitBreakerEvent::CallPermitted {
                                pattern_name: config.name.clone(),
                                timestamp: Instant::now(),
                                state: self.state,
                            });
                        Admission::Admitted(Some(permit))
                    }
                    Err(_) => {
                        self.record_rejection(config);
                        Admission::Rejected
                    }
                }
            }
        }
    }

    pub(crate) fn record_rejection<C>(&self, config: &CircuitBreakerConfig<C>) {
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::CallRejected {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
            });
    }

    /// Read-only check of whether a call would be permitted.
    ///
    /// Returns `Ok(())` if a call would likely succeed, or `Err(duration)` with
    /// the suggested wait time before retrying.
    ///
    /// Unlike `try_acquire`, this does not mutate state or emit events.
    pub fn check_permitted<C>(&self, config: &CircuitBreakerConfig<C>) -> Result<(), Duration> {
        match self.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                if config.manual_mode {
                    // The recovery timer is disabled in manual mode; report
                    // the configured wait as a heartbeat interval for
                    // backpressure callers, but the circuit will not open up
                    // on its own no matter how long they wait.
                    return Err(config.wait_duration_in_open);
                }
                let elapsed = self.last_state_change.elapsed();
                if elapsed >= config.wait_duration_in_open {
                    // Wait has elapsed; try_acquire will transition to HalfOpen
                    Ok(())
                } else {
                    Err(config.wait_duration_in_open - elapsed)
                }
            }
            CircuitState::HalfOpen => {
                if self.half_open_permits.available_permits() > 0 {
                    Ok(())
                } else {
                    // Half-open batch is fully reserved. A slot can free up
                    // at any time -- whenever an admitted-but-incomplete
                    // probe is cancelled -- so this is not tied to the
                    // open-circuit cooldown timer. Use a short, bounded
                    // retry instead, the same pattern `poll_circuit_gate`
                    // already uses for mutex contention, so `poll_ready`
                    // stays responsive without busy-spinning.
                    Err(Duration::from_millis(1))
                }
            }
        }
    }

    pub fn force_open<C>(&mut self, config: &CircuitBreakerConfig<C>) {
        self.transition_to(CircuitState::Open, config);
    }

    pub fn force_closed<C>(&mut self, config: &CircuitBreakerConfig<C>) {
        self.transition_to(CircuitState::Closed, config);
    }

    pub fn reset<C>(&mut self, config: &CircuitBreakerConfig<C>) {
        self.transition_to(CircuitState::Closed, config);
    }

    fn transition_to<C>(&mut self, state: CircuitState, config: &CircuitBreakerConfig<C>) {
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
        self.consecutive_failures = 0;

        if state == CircuitState::HalfOpen {
            // Start this half-open cycle with a full, fresh batch of trial
            // slots. See the `half_open_permits` field doc for why this
            // must be a new `Arc<Semaphore>` rather than a resized existing
            // one.
            self.half_open_permits = Arc::new(Semaphore::new(config.permitted_calls_in_half_open));
        }
    }

    fn evaluate_window<C>(&mut self, config: &CircuitBreakerConfig<C>) {
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

        // Slow-call detection runs in both failure models -- a circuit that
        // is healthy by error rate but degraded by latency should still open.
        // Sliding-window gating still applies to the slow-call evaluation.
        let slow_should_open = config.slow_call_duration_threshold.is_some()
            && total_count >= config.minimum_number_of_calls
            && !(config.sliding_window_type == SlidingWindowType::CountBased
                && total_count < config.sliding_window_size)
            && {
                let slow_call_rate = slow_call_count as f64 / total_count as f64;
                slow_call_rate >= config.slow_call_rate_threshold
            };

        let failure_should_open = match config.failure_model {
            FailureModel::SlidingWindow => {
                // Don't evaluate until minimum calls threshold is met
                if total_count < config.minimum_number_of_calls {
                    false
                } else if config.sliding_window_type == SlidingWindowType::CountBased
                    && total_count < config.sliding_window_size
                {
                    // For count-based window, also wait until window is full
                    false
                } else {
                    let failure_rate = failure_count as f64 / total_count as f64;
                    failure_rate >= config.failure_rate_threshold
                }
            }
            FailureModel::ConsecutiveFailures { k } => self.consecutive_failures >= k,
        };

        if failure_should_open || slow_should_open {
            self.transition_to(CircuitState::Open, config);
        }
        // Don't transition to closed if we're in HalfOpen - that happens via record_success
    }
}
