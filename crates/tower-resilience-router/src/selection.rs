//! Selection strategies for weighted routing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Strategy for selecting which backend handles a request.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Deterministic round-robin weighted selection (default).
    ///
    /// Uses smooth weighted round-robin to distribute requests in a
    /// predictable, repeatable pattern. With weights `[90, 10]`, every
    /// normalized cycle of 10 requests sends exactly 9 to the first backend
    /// and 1 to the second, with the canary request spread through the cycle
    /// instead of placed in a contiguous bucket.
    ///
    /// Each selection briefly locks and scans the shared backend scores. No
    /// backend or listener code runs while the lock is held.
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
///
/// The progression state is reference-counted so every clone of a router
/// participates in the same selection sequence.
pub(crate) struct WeightedSelector {
    /// Individual weights used by smooth weighted round-robin.
    weights: Arc<[u32]>,
    /// Cumulative weights for binary search selection.
    cumulative_weights: Arc<[u64]>,
    /// Total weight across all backends.
    total_weight: u64,
    /// Shared progression for the configured strategy.
    progression: Arc<Progression>,
}

/// Mutable selection state shared by every selector clone.
enum Progression {
    /// Current scores for classic smooth weighted round-robin.
    Deterministic(Mutex<Vec<i128>>),
    /// Counter mixed into a deterministic pseudo-random sequence.
    Random(AtomicU64),
}

impl WeightedSelector {
    /// Creates a new selector from weights.
    pub(crate) fn new(weights: &[u32], strategy: SelectionStrategy) -> Self {
        assert!(!weights.is_empty(), "at least one backend is required");

        let mut cumulative_weights = Vec::with_capacity(weights.len());
        let mut cumulative = 0u64;
        for (index, &w) in weights.iter().enumerate() {
            assert!(
                w > 0,
                "backend {index} has weight 0; all weights must be positive"
            );
            cumulative = cumulative
                .checked_add(u64::from(w))
                .expect("sum of backend weights exceeds u64::MAX");
            cumulative_weights.push(cumulative);
        }

        let progression = match strategy {
            SelectionStrategy::Deterministic => {
                Progression::Deterministic(Mutex::new(vec![0; weights.len()]))
            }
            SelectionStrategy::Random => Progression::Random(AtomicU64::new(0)),
        };

        Self {
            total_weight: cumulative,
            weights: Arc::from(weights),
            cumulative_weights: Arc::from(cumulative_weights),
            progression: Arc::new(progression),
        }
    }

    /// Selects a backend index.
    pub(crate) fn select(&self) -> usize {
        match self.progression.as_ref() {
            Progression::Deterministic(current_weights) => self.select_smooth(current_weights),
            Progression::Random(counter) => {
                // Simple LCG-based random: fast, no external dependency.
                // We use the counter as seed state for reproducibility in tests
                // when needed, but each call advances it.
                let count = counter.fetch_add(1, Ordering::Relaxed);
                // Mix bits using a simple hash to get pseudo-random distribution
                let hash = count
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                self.bucket_for(hash % self.total_weight)
            }
        }
    }

    /// Advances classic smooth weighted round-robin and returns the winner.
    fn select_smooth(&self, current_weights: &Mutex<Vec<i128>>) -> usize {
        // No user code runs while this lock is held. Recovering poison is safe:
        // all score updates below are infallible and maintain a zero-sum state.
        let mut current = current_weights
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for (&weight, score) in self.weights.iter().zip(current.iter_mut()) {
            *score += i128::from(weight);
        }

        let mut selected = 0;
        for index in 1..current.len() {
            if current[index] > current[selected] {
                selected = index;
            }
        }

        current[selected] -= i128::from(self.total_weight);
        selected
    }

    /// Maps a point in `0..total_weight` to its cumulative-weight bucket.
    fn bucket_for(&self, point: u64) -> usize {
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
            weights: Arc::clone(&self.weights),
            cumulative_weights: Arc::clone(&self.cumulative_weights),
            total_weight: self.total_weight,
            progression: Arc::clone(&self.progression),
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

        let first_cycle: Vec<usize> = (0..10).map(|_| selector.select()).collect();
        let second_cycle: Vec<usize> = (0..10).map(|_| selector.select()).collect();

        assert_eq!(first_cycle, second_cycle);
    }

    #[test]
    fn deterministic_spreads_small_weight_through_cycle() {
        let selector = WeightedSelector::new(&[9, 1], SelectionStrategy::Deterministic);

        let cycle: Vec<usize> = (0..10).map(|_| selector.select()).collect();

        assert_eq!(cycle, vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn clones_share_deterministic_progression() {
        let selector = WeightedSelector::new(&[3, 1], SelectionStrategy::Deterministic);

        let picks: Vec<usize> = (0..8).map(|_| selector.clone().select()).collect();

        assert_eq!(picks, vec![0, 0, 1, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn clones_share_random_progression() {
        let selector = WeightedSelector::new(&[1, 1, 1, 1], SelectionStrategy::Random);

        let picks: Vec<usize> = (0..4).map(|_| selector.clone().select()).collect();

        assert_eq!(picks, vec![3, 0, 1, 2]);
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

    #[test]
    fn maximum_weights_do_not_overflow() {
        let selector = WeightedSelector::new(
            &[u32::MAX, u32::MAX, u32::MAX],
            SelectionStrategy::Deterministic,
        );

        let mut counts = [0u32; 3];
        for _ in 0..300 {
            counts[selector.select()] += 1;
        }

        assert_eq!(counts, [100, 100, 100]);
    }

    #[test]
    fn random_counter_wraps_without_invalid_selection() {
        let selector = WeightedSelector::new(&[3, 1], SelectionStrategy::Random);
        let Progression::Random(counter) = selector.progression.as_ref() else {
            panic!("expected random progression");
        };
        counter.store(u64::MAX, Ordering::Relaxed);

        assert!(selector.select() < 2);
        assert!(selector.select() < 2);
    }
}
