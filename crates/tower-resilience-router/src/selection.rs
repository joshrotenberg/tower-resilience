//! Selection strategies for weighted routing.

use std::sync::atomic::{AtomicU64, Ordering};

/// Strategy for selecting which backend handles a request.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Deterministic round-robin weighted selection (default).
    ///
    /// Uses an atomic counter to distribute requests in a predictable,
    /// repeatable pattern. With weights `[90, 10]`, every cycle of 100
    /// requests sends exactly 90 to the first backend and 10 to the second.
    ///
    /// This is the recommended default for canary deployments because:
    /// - Behavior is predictable at any traffic volume
    /// - Easy to test and debug ("request N went to backend X")
    /// - No variance issues at low request rates
    #[default]
    Deterministic,

    /// Random weighted selection.
    ///
    /// Each request independently selects a backend with probability
    /// proportional to its weight. Over many requests the distribution
    /// converges to the configured weights, but short-term variance
    /// is possible -- especially at low traffic volumes.
    Random,
}

/// Selector that picks a backend index based on weights and strategy.
pub(crate) struct WeightedSelector {
    /// Cumulative weights for binary search selection.
    cumulative_weights: Vec<u64>,
    /// Total weight across all backends.
    total_weight: u64,
    /// Atomic counter for deterministic selection.
    counter: AtomicU64,
    /// The strategy to use.
    strategy: SelectionStrategy,
}

impl WeightedSelector {
    /// Creates a new selector from weights.
    pub(crate) fn new(weights: &[u32], strategy: SelectionStrategy) -> Self {
        let mut cumulative_weights = Vec::with_capacity(weights.len());
        let mut cumulative = 0u64;
        for &w in weights {
            cumulative += u64::from(w);
            cumulative_weights.push(cumulative);
        }

        Self {
            total_weight: cumulative,
            cumulative_weights,
            counter: AtomicU64::new(0),
            strategy,
        }
    }

    /// Selects a backend index.
    pub(crate) fn select(&self) -> usize {
        let point = match self.strategy {
            SelectionStrategy::Deterministic => {
                let count = self.counter.fetch_add(1, Ordering::Relaxed);
                count % self.total_weight
            }
            SelectionStrategy::Random => {
                // Simple LCG-based random: fast, no external dependency.
                // We use the counter as seed state for reproducibility in tests
                // when needed, but each call advances it.
                let count = self.counter.fetch_add(1, Ordering::Relaxed);
                // Mix bits using a simple hash to get pseudo-random distribution
                let hash = count
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                hash % self.total_weight
            }
        };

        // Binary search for the bucket this point falls into
        match self.cumulative_weights.binary_search(&(point + 1)) {
            Ok(idx) => idx,
            Err(idx) => idx,
        }
    }
}

impl Clone for WeightedSelector {
    fn clone(&self) -> Self {
        Self {
            cumulative_weights: self.cumulative_weights.clone(),
            total_weight: self.total_weight,
            counter: AtomicU64::new(self.counter.load(Ordering::Relaxed)),
            strategy: self.strategy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_distributes_exactly() {
        let selector = WeightedSelector::new(&[90, 10], SelectionStrategy::Deterministic);

        let mut counts = [0u32; 2];
        for _ in 0..100 {
            let idx = selector.select();
            counts[idx] += 1;
        }

        assert_eq!(counts[0], 90);
        assert_eq!(counts[1], 10);
    }

    #[test]
    fn deterministic_repeats_cycle() {
        let selector = WeightedSelector::new(&[70, 30], SelectionStrategy::Deterministic);

        let first_cycle: Vec<usize> = (0..100).map(|_| selector.select()).collect();

        // Reset counter
        selector.counter.store(0, Ordering::Relaxed);
        let second_cycle: Vec<usize> = (0..100).map(|_| selector.select()).collect();

        assert_eq!(first_cycle, second_cycle);
    }

    #[test]
    fn random_converges_to_weights() {
        let selector = WeightedSelector::new(&[80, 20], SelectionStrategy::Random);

        let mut counts = [0u32; 2];
        let total = 10_000;
        for _ in 0..total {
            let idx = selector.select();
            counts[idx] += 1;
        }

        // Allow 5% tolerance
        let ratio = f64::from(counts[0]) / f64::from(total);
        assert!(
            (0.75..=0.85).contains(&ratio),
            "expected ~80%, got {:.1}%",
            ratio * 100.0
        );
    }

    #[test]
    fn single_backend() {
        let selector = WeightedSelector::new(&[1], SelectionStrategy::Deterministic);

        for _ in 0..100 {
            assert_eq!(selector.select(), 0);
        }
    }

    #[test]
    fn three_backends() {
        let selector = WeightedSelector::new(&[50, 30, 20], SelectionStrategy::Deterministic);

        let mut counts = [0u32; 3];
        for _ in 0..100 {
            let idx = selector.select();
            counts[idx] += 1;
        }

        assert_eq!(counts[0], 50);
        assert_eq!(counts[1], 30);
        assert_eq!(counts[2], 20);
    }
}
