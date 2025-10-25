//! Selection strategies for choosing healthy resources.

use crate::{HealthCheckedContext, HealthStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Type alias for custom selector function
pub type CustomSelectorFn = Arc<dyn Fn(&[HealthStatus]) -> Option<usize> + Send + Sync>;

/// Trait for implementing custom selection strategies.
///
/// # Examples
///
/// ```rust
/// use tower_resilience_healthcheck::{Selector, HealthCheckedContext};
///
/// struct FirstHealthySelector;
///
/// impl<T> Selector<T> for FirstHealthySelector {
///     fn select<'a>(&self, contexts: &'a [HealthCheckedContext<T>]) -> Option<usize> {
///         contexts.iter()
///             .position(|ctx| ctx.status().is_healthy())
///     }
/// }
/// ```
pub trait Selector<T>: Send + Sync {
    /// Select a context from the available options.
    ///
    /// Returns the index of the selected context, or `None` if no suitable context is available.
    fn select(&self, contexts: &[HealthCheckedContext<T>]) -> Option<usize>;
}

// Blanket impl for closures
impl<T, F> Selector<T> for F
where
    F: Fn(&[HealthCheckedContext<T>]) -> Option<usize> + Send + Sync,
{
    fn select(&self, contexts: &[HealthCheckedContext<T>]) -> Option<usize> {
        self(contexts)
    }
}

/// Built-in selection strategies.
#[derive(Clone, Default)]
pub enum SelectionStrategy {
    /// Return first available healthy resource.
    /// Best for: Primary/secondary failover scenarios.
    #[default]
    FirstAvailable,

    /// Return a random healthy resource.
    /// Best for: Load distribution across multiple instances.
    #[cfg(feature = "random")]
    Random,

    /// Round-robin through healthy resources.
    /// Best for: Even distribution of load.
    RoundRobin,

    /// Prefer healthy resources, fallback to degraded if no healthy ones available.
    /// Best for: Accepting degraded performance over failure.
    PreferHealthy,

    /// Use a custom selector implementation.
    /// Best for: Complex selection logic (latency-based, weighted, etc.).
    Custom(CustomSelectorFn),
}

impl SelectionStrategy {
    /// Select a context index based on the strategy.
    ///
    /// This is an internal helper that takes a slice of statuses instead of full contexts
    /// to avoid cloning during selection.
    pub(crate) fn select<T>(
        &self,
        contexts: &[HealthCheckedContext<T>],
        round_robin_counter: &AtomicUsize,
    ) -> Option<usize> {
        if contexts.is_empty() {
            return None;
        }

        // Collect statuses for efficient filtering
        let statuses: Vec<HealthStatus> = contexts.iter().map(|ctx| ctx.status()).collect();

        match self {
            SelectionStrategy::FirstAvailable => statuses.iter().position(|s| s.is_usable()),

            #[cfg(feature = "random")]
            SelectionStrategy::Random => {
                let usable: Vec<usize> = statuses
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.is_usable())
                    .map(|(i, _)| i)
                    .collect();

                if usable.is_empty() {
                    None
                } else {
                    use rand::Rng;
                    let idx = rand::rng().random_range(0..usable.len());
                    Some(usable[idx])
                }
            }

            SelectionStrategy::RoundRobin => {
                let usable: Vec<usize> = statuses
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.is_usable())
                    .map(|(i, _)| i)
                    .collect();

                if usable.is_empty() {
                    None
                } else {
                    let idx = round_robin_counter.fetch_add(1, Ordering::Relaxed);
                    Some(usable[idx % usable.len()])
                }
            }

            SelectionStrategy::PreferHealthy => {
                // Try to find healthy first
                statuses
                    .iter()
                    .position(|s| *s == HealthStatus::Healthy)
                    .or_else(|| statuses.iter().position(|s| s.is_usable()))
            }

            SelectionStrategy::Custom(selector) => selector(&statuses),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HealthCheckedContext;

    fn create_context(name: &str, status: HealthStatus) -> HealthCheckedContext<String> {
        let ctx = HealthCheckedContext::new(name.to_string(), name);
        ctx.set_status(status);
        ctx
    }

    #[test]
    fn test_first_available() {
        let contexts = vec![
            create_context("unhealthy", HealthStatus::Unhealthy),
            create_context("healthy", HealthStatus::Healthy),
            create_context("degraded", HealthStatus::Degraded),
        ];

        let strategy = SelectionStrategy::FirstAvailable;
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, Some(1)); // First healthy
    }

    #[test]
    fn test_first_available_with_degraded() {
        let contexts = vec![
            create_context("unhealthy", HealthStatus::Unhealthy),
            create_context("degraded", HealthStatus::Degraded),
        ];

        let strategy = SelectionStrategy::FirstAvailable;
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, Some(1)); // Degraded is usable
    }

    #[test]
    fn test_round_robin() {
        let contexts = vec![
            create_context("healthy1", HealthStatus::Healthy),
            create_context("healthy2", HealthStatus::Healthy),
            create_context("healthy3", HealthStatus::Healthy),
        ];

        let strategy = SelectionStrategy::RoundRobin;
        let counter = AtomicUsize::new(0);

        let first = strategy.select(&contexts, &counter);
        let second = strategy.select(&contexts, &counter);
        let third = strategy.select(&contexts, &counter);
        let fourth = strategy.select(&contexts, &counter);

        assert_eq!(first, Some(0));
        assert_eq!(second, Some(1));
        assert_eq!(third, Some(2));
        assert_eq!(fourth, Some(0)); // Wraps around
    }

    #[test]
    fn test_round_robin_with_unhealthy() {
        let contexts = vec![
            create_context("healthy1", HealthStatus::Healthy),
            create_context("unhealthy", HealthStatus::Unhealthy),
            create_context("healthy2", HealthStatus::Healthy),
        ];

        let strategy = SelectionStrategy::RoundRobin;
        let counter = AtomicUsize::new(0);

        let first = strategy.select(&contexts, &counter);
        let second = strategy.select(&contexts, &counter);
        let third = strategy.select(&contexts, &counter);

        assert_eq!(first, Some(0));
        assert_eq!(second, Some(2)); // Skips unhealthy
        assert_eq!(third, Some(0)); // Wraps
    }

    #[test]
    fn test_prefer_healthy() {
        let contexts = vec![
            create_context("degraded", HealthStatus::Degraded),
            create_context("healthy", HealthStatus::Healthy),
        ];

        let strategy = SelectionStrategy::PreferHealthy;
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, Some(1)); // Prefers healthy over degraded
    }

    #[test]
    fn test_prefer_healthy_fallback_to_degraded() {
        let contexts = vec![
            create_context("unhealthy", HealthStatus::Unhealthy),
            create_context("degraded", HealthStatus::Degraded),
        ];

        let strategy = SelectionStrategy::PreferHealthy;
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, Some(1)); // Falls back to degraded
    }

    #[test]
    fn test_no_usable_resources() {
        let contexts = vec![
            create_context("unhealthy1", HealthStatus::Unhealthy),
            create_context("unhealthy2", HealthStatus::Unhealthy),
        ];

        let strategy = SelectionStrategy::FirstAvailable;
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, None);
    }

    #[test]
    fn test_custom_selector() {
        let contexts = vec![
            create_context("first", HealthStatus::Healthy),
            create_context("second", HealthStatus::Healthy),
        ];

        // Custom selector that always picks the last healthy resource
        let selector = Arc::new(|statuses: &[HealthStatus]| {
            statuses
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_healthy())
                .next_back()
                .map(|(i, _)| i)
        });

        let strategy = SelectionStrategy::Custom(selector);
        let counter = AtomicUsize::new(0);

        let selected = strategy.select(&contexts, &counter);
        assert_eq!(selected, Some(1)); // Last healthy
    }
}
