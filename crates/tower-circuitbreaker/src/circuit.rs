use crate::config::CircuitBreakerConfig;
use crate::events::CircuitBreakerEvent;
#[cfg(feature = "metrics")]
use metrics::{counter, gauge};
use std::time::Instant;

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

        // Emit event
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::SuccessRecorded {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                state: self.state,
            });

        #[cfg(feature = "metrics")]
        counter!("circuitbreaker_calls_total", "outcome" => "success").increment(1);

        match self.state {
            CircuitState::HalfOpen => {
                if self.success_count >= config.permitted_calls_in_half_open {
                    self.transition_to(CircuitState::Closed, config);
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

        // Emit event
        config
            .event_listeners
            .emit(&CircuitBreakerEvent::FailureRecorded {
                pattern_name: config.name.clone(),
                timestamp: Instant::now(),
                state: self.state,
            });

        #[cfg(feature = "metrics")]
        counter!("circuitbreaker_calls_total", "outcome" => "failure").increment(1);

        match self.state {
            CircuitState::HalfOpen => {
                self.transition_to(CircuitState::Open, config);
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
            self.transition_to(CircuitState::Open, config);
        } else {
            self.transition_to(CircuitState::Closed, config);
        }
    }
}
