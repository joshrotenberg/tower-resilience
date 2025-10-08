use crate::config::CircuitBreakerConfig;
#[cfg(feature = "metrics")]
use metrics::{counter, gauge};

/// Represents the state of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// The circuit is closed and calls are allowed.
    Closed,
    /// The circuit is open and calls are rejected.
    Open,
    /// The circuit is half-open and a limited number of calls are allowed.
    HalfOpen,
}

pub(crate) struct Circuit {
    state: CircuitState,
    last_state_change: std::time::Instant,
    failure_count: usize,
    success_count: usize,
    total_count: usize,
}

impl Default for Circuit {
    fn default() -> Self {
        Self {
            state: CircuitState::Closed,
            last_state_change: std::time::Instant::now(),
            failure_count: 0,
            success_count: 0,
            total_count: 0,
        }
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    pub fn record_success(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        self.success_count += 1;
        self.total_count += 1;

        #[cfg(feature = "metrics")]
        counter!("circuitbreaker_calls_total", "outcome" => "success").increment(1);

        match self.state {
            CircuitState::HalfOpen => {
                if self.success_count >= config.permitted_calls_in_half_open {
                    self.transition_to(CircuitState::Closed);
                }
            }
            _ => {
                if self.total_count >= config.sliding_window_size {
                    self.evaluate_window(config);
                }
            }
        }
    }

    pub fn record_failure(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        self.failure_count += 1;
        self.total_count += 1;

        #[cfg(feature = "metrics")]
        counter!("circuitbreaker_calls_total", "outcome" => "failure").increment(1);

        match self.state {
            CircuitState::HalfOpen => {
                self.transition_to(CircuitState::Open);
            }
            _ => {
                if self.total_count >= config.sliding_window_size {
                    self.evaluate_window(config);
                }
            }
        }
    }

    pub fn try_acquire(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if self.last_state_change.elapsed() >= config.wait_duration_in_open {
                    self.transition_to(CircuitState::HalfOpen);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                self.success_count + self.failure_count < config.permitted_calls_in_half_open
            }
        }
    }

    pub fn force_open(&mut self) {
        self.transition_to(CircuitState::Open);
    }

    pub fn force_closed(&mut self) {
        self.transition_to(CircuitState::Closed);
    }

    pub fn reset(&mut self) {
        self.transition_to(CircuitState::Closed);
    }

    fn transition_to(&mut self, state: CircuitState) {
        #[cfg(feature = "tracing")]
        if self.state != state {
            tracing::info!(from = ?self.state, to = ?state, "Circuit state transition");
        }

        #[cfg(feature = "metrics")]
        {
            counter!(
                "circuitbreaker_transitions_total",
                "from" => match self.state {
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

            gauge!("circuitbreaker_state", "state" => match state {
                CircuitState::Closed => "Closed",
                CircuitState::Open => "Open",
                CircuitState::HalfOpen => "HalfOpen",
            })
            .set(1.0);
        }

        self.state = state;
        self.last_state_change = std::time::Instant::now();
        self.success_count = 0;
        self.failure_count = 0;
        self.total_count = 0;
    }

    fn evaluate_window(&mut self, config: &CircuitBreakerConfig<impl Sized, impl Sized>) {
        if self.total_count < config.minimum_number_of_calls {
            return;
        }

        let failure_rate = self.failure_count as f64 / self.total_count as f64;
        if failure_rate >= config.failure_rate_threshold {
            self.transition_to(CircuitState::Open);
        } else {
            self.transition_to(CircuitState::Closed);
        }
    }
}
