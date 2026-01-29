//! Health check integration for proactive circuit breaker control.
//!
//! This module provides `HealthTriggerable` implementations that allow
//! health check systems to proactively open or close the circuit breaker
//! based on external health signals.

use crate::{CircuitBreaker, CircuitBreakerWithFallback};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_resilience_core::HealthTriggerable;

use crate::circuit::Circuit;
use crate::config::CircuitBreakerConfig;

/// Internal helper to implement triggering on circuit state.
fn trigger_unhealthy_impl<C>(circuit: Arc<Mutex<Circuit>>, config: Arc<CircuitBreakerConfig<C>>)
where
    C: Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut circuit = circuit.lock().await;
        circuit.force_open(&config);
    });
}

/// Internal helper to implement triggering on circuit state.
fn trigger_healthy_impl<C>(circuit: Arc<Mutex<Circuit>>, config: Arc<CircuitBreakerConfig<C>>)
where
    C: Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut circuit = circuit.lock().await;
        circuit.force_closed(&config);
    });
}

impl<S, C> HealthTriggerable for CircuitBreaker<S, C>
where
    S: Send + Sync + 'static,
    C: Send + Sync + 'static,
{
    fn trigger_unhealthy(&self) {
        trigger_unhealthy_impl(Arc::clone(&self.circuit), Arc::clone(&self.config));
    }

    fn trigger_healthy(&self) {
        trigger_healthy_impl(Arc::clone(&self.circuit), Arc::clone(&self.config));
    }
}

impl<S, C, Req, Res, Err> HealthTriggerable for CircuitBreakerWithFallback<S, C, Req, Res, Err>
where
    S: Send + Sync + 'static,
    C: Send + Sync + 'static,
    Req: Send + Sync + 'static,
    Res: Send + Sync + 'static,
    Err: Send + Sync + 'static,
{
    fn trigger_unhealthy(&self) {
        trigger_unhealthy_impl(Arc::clone(&self.circuit), Arc::clone(&self.config));
    }

    fn trigger_healthy(&self) {
        trigger_healthy_impl(Arc::clone(&self.circuit), Arc::clone(&self.config));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::DefaultClassifier;
    use crate::CircuitState;
    use std::time::Duration;
    use tower_resilience_core::EventListeners;

    fn dummy_config() -> CircuitBreakerConfig<DefaultClassifier> {
        CircuitBreakerConfig {
            failure_rate_threshold: 0.5,
            sliding_window_type: crate::config::SlidingWindowType::CountBased,
            sliding_window_size: 10,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(60),
            permitted_calls_in_half_open: 1,
            failure_classifier: DefaultClassifier,
            minimum_number_of_calls: 10,
            slow_call_duration_threshold: None,
            slow_call_rate_threshold: 1.0,
            event_listeners: EventListeners::new(),
            name: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_health_triggerable_opens_circuit() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        // Initially closed
        assert_eq!(breaker.state_sync(), CircuitState::Closed);

        // Trigger unhealthy
        breaker.trigger_unhealthy();

        // Wait for the spawned task to complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should be open now
        assert_eq!(breaker.state_sync(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_health_triggerable_closes_circuit() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        // Force open first
        breaker.force_open().await;
        assert_eq!(breaker.state_sync(), CircuitState::Open);

        // Trigger healthy
        breaker.trigger_healthy();

        // Wait for the spawned task to complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should be closed now
        assert_eq!(breaker.state_sync(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_health_triggerable_via_trait_object() {
        let config = Arc::new(dummy_config());
        let breaker: CircuitBreaker<(), DefaultClassifier> = CircuitBreaker::new((), config);

        // Use via trait object (like health check would)
        let trigger: Arc<dyn HealthTriggerable> = Arc::new(breaker.clone());

        trigger.trigger_unhealthy();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(breaker.state_sync(), CircuitState::Open);

        trigger.trigger_healthy();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(breaker.state_sync(), CircuitState::Closed);
    }
}
