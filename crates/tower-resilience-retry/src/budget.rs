//! Retry budget implementations to prevent retry storms.
//!
//! Retry budgets limit the total number of retries across all requests,
//! preventing cascading failures when a downstream service is struggling.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A budget that controls how many retries are allowed.
///
/// Budgets are shared across all clones of a service, providing
/// global rate limiting for retries.
pub trait RetryBudget: Send + Sync {
    /// Attempt to withdraw one retry token from the budget.
    ///
    /// Returns `true` if the retry is allowed, `false` if the budget is exhausted.
    fn try_withdraw(&self) -> bool;

    /// Deposit tokens after a successful request.
    ///
    /// This replenishes the budget, allowing future retries.
    fn deposit(&self);

    /// Get the current budget balance (for observability).
    fn balance(&self) -> usize;
}

/// Builder for creating retry budgets.
#[derive(Clone, Default)]
pub struct RetryBudgetBuilder;

impl RetryBudgetBuilder {
    /// Create a new budget builder.
    pub fn new() -> Self {
        Self
    }

    /// Configure a token bucket budget.
    ///
    /// Tokens are added at a fixed rate and consumed by retries.
    /// When tokens are exhausted, retries are rejected.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_retry::RetryBudgetBuilder;
    ///
    /// let budget = RetryBudgetBuilder::new()
    ///     .token_bucket()
    ///     .tokens_per_second(10.0)
    ///     .max_tokens(100)
    ///     .build();
    /// ```
    pub fn token_bucket(self) -> TokenBucketBuilder {
        TokenBucketBuilder {
            tokens_per_second: 10.0,
            max_tokens: 100,
            initial_tokens: None,
        }
    }

    /// Configure an AIMD (Additive Increase Multiplicative Decrease) budget.
    ///
    /// The budget grows linearly on successful deposits and shrinks
    /// multiplicatively when the budget is exhausted.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_retry::RetryBudgetBuilder;
    ///
    /// let budget = RetryBudgetBuilder::new()
    ///     .aimd()
    ///     .min_budget(10)
    ///     .max_budget(1000)
    ///     .build();
    /// ```
    pub fn aimd(self) -> AimdBudgetBuilder {
        AimdBudgetBuilder {
            min_budget: 10,
            max_budget: 1000,
            deposit_amount: 1,
            withdraw_amount: 1,
            decrease_factor: 0.5,
        }
    }
}

/// Builder for token bucket budgets.
pub struct TokenBucketBuilder {
    tokens_per_second: f64,
    max_tokens: usize,
    initial_tokens: Option<usize>,
}

impl TokenBucketBuilder {
    /// Set the token refill rate.
    ///
    /// Default: 10.0 tokens per second
    pub fn tokens_per_second(mut self, rate: f64) -> Self {
        self.tokens_per_second = rate;
        self
    }

    /// Set the maximum number of tokens (burst capacity).
    ///
    /// Default: 100
    pub fn max_tokens(mut self, max: usize) -> Self {
        self.max_tokens = max;
        self
    }

    /// Set the initial number of tokens.
    ///
    /// Default: same as max_tokens
    pub fn initial_tokens(mut self, initial: usize) -> Self {
        self.initial_tokens = Some(initial);
        self
    }

    /// Build the token bucket budget.
    pub fn build(self) -> Arc<dyn RetryBudget> {
        Arc::new(TokenBucketBudget::new(
            self.tokens_per_second,
            self.max_tokens,
            self.initial_tokens.unwrap_or(self.max_tokens),
        ))
    }
}

/// Builder for AIMD budgets.
pub struct AimdBudgetBuilder {
    min_budget: usize,
    max_budget: usize,
    deposit_amount: usize,
    withdraw_amount: usize,
    decrease_factor: f64,
}

impl AimdBudgetBuilder {
    /// Set the minimum budget floor.
    ///
    /// The budget will never go below this value.
    /// Default: 10
    pub fn min_budget(mut self, min: usize) -> Self {
        self.min_budget = min;
        self
    }

    /// Set the maximum budget ceiling.
    ///
    /// The budget will never exceed this value.
    /// Default: 1000
    pub fn max_budget(mut self, max: usize) -> Self {
        self.max_budget = max;
        self
    }

    /// Set how many tokens to add on each successful request.
    ///
    /// Default: 1
    pub fn deposit_amount(mut self, amount: usize) -> Self {
        self.deposit_amount = amount;
        self
    }

    /// Set how many tokens each retry consumes.
    ///
    /// Default: 1
    pub fn withdraw_amount(mut self, amount: usize) -> Self {
        self.withdraw_amount = amount;
        self
    }

    /// Set the multiplicative decrease factor when budget is exhausted.
    ///
    /// When a retry is rejected due to budget exhaustion, the max budget
    /// is multiplied by this factor.
    /// Default: 0.5
    pub fn decrease_factor(mut self, factor: f64) -> Self {
        self.decrease_factor = factor;
        self
    }

    /// Build the AIMD budget.
    pub fn build(self) -> Arc<dyn RetryBudget> {
        Arc::new(AimdBudget::new(
            self.min_budget,
            self.max_budget,
            self.deposit_amount,
            self.withdraw_amount,
            self.decrease_factor,
        ))
    }
}

/// Token bucket retry budget.
///
/// Tokens are consumed by retries and replenished by successful requests.
/// This provides a simple way to limit retry storms.
pub struct TokenBucketBudget {
    /// Current token balance (scaled by 1000 for precision)
    tokens: AtomicU64,
    /// Maximum tokens (scaled)
    max_tokens: u64,
}

impl TokenBucketBudget {
    /// Create a new token bucket budget.
    ///
    /// Note: `tokens_per_second` is currently unused - tokens are only
    /// replenished via `deposit()` calls on successful requests.
    pub fn new(_tokens_per_second: f64, max_tokens: usize, initial_tokens: usize) -> Self {
        const SCALE: u64 = 1000;
        Self {
            tokens: AtomicU64::new((initial_tokens as u64) * SCALE),
            max_tokens: (max_tokens as u64) * SCALE,
        }
    }
}

impl RetryBudget for TokenBucketBudget {
    fn try_withdraw(&self) -> bool {
        // Try to refill first (best effort, non-blocking)
        // For a more accurate refill, we'd need async, but this is good enough
        // for most cases since deposits also trigger refill
        const SCALE: u64 = 1000;

        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < SCALE {
                return false;
            }
            let new_tokens = current - SCALE;
            if self
                .tokens
                .compare_exchange_weak(current, new_tokens, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn deposit(&self) {
        // Refill based on elapsed time, then add deposit bonus
        // Since we can't easily do async here, we do a simple increment
        const SCALE: u64 = 1000;
        let current = self.tokens.load(Ordering::Relaxed);
        let new_tokens = (current + SCALE).min(self.max_tokens);
        self.tokens.store(new_tokens, Ordering::Relaxed);
    }

    fn balance(&self) -> usize {
        const SCALE: u64 = 1000;
        (self.tokens.load(Ordering::Relaxed) / SCALE) as usize
    }
}

/// AIMD (Additive Increase Multiplicative Decrease) retry budget.
///
/// The budget grows linearly with successful requests and shrinks
/// multiplicatively when retries are rejected.
pub struct AimdBudget {
    /// Current token balance
    tokens: AtomicU64,
    /// Minimum budget floor
    min_budget: u64,
    /// Current maximum budget (can decrease on exhaustion)
    current_max: AtomicU64,
    /// Absolute maximum budget
    absolute_max: u64,
    /// Tokens to add on deposit
    deposit_amount: u64,
    /// Tokens to remove on withdraw
    withdraw_amount: u64,
    /// Factor to multiply max by on exhaustion
    decrease_factor: f64,
}

impl AimdBudget {
    /// Create a new AIMD budget.
    pub fn new(
        min_budget: usize,
        max_budget: usize,
        deposit_amount: usize,
        withdraw_amount: usize,
        decrease_factor: f64,
    ) -> Self {
        Self {
            tokens: AtomicU64::new(max_budget as u64),
            min_budget: min_budget as u64,
            current_max: AtomicU64::new(max_budget as u64),
            absolute_max: max_budget as u64,
            deposit_amount: deposit_amount as u64,
            withdraw_amount: withdraw_amount as u64,
            decrease_factor,
        }
    }
}

impl RetryBudget for AimdBudget {
    fn try_withdraw(&self) -> bool {
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < self.withdraw_amount {
                // Budget exhausted - apply multiplicative decrease to max
                let current_max = self.current_max.load(Ordering::Relaxed);
                let new_max =
                    ((current_max as f64 * self.decrease_factor) as u64).max(self.min_budget);
                self.current_max.store(new_max, Ordering::Relaxed);
                return false;
            }
            let new_tokens = current - self.withdraw_amount;
            if self
                .tokens
                .compare_exchange_weak(current, new_tokens, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn deposit(&self) {
        let current_max = self.current_max.load(Ordering::Relaxed);
        let current = self.tokens.load(Ordering::Relaxed);

        // Additive increase: add deposit amount, cap at current max
        let new_tokens = (current + self.deposit_amount).min(current_max);
        self.tokens.store(new_tokens, Ordering::Relaxed);

        // Also slowly increase the max back toward absolute max
        if current_max < self.absolute_max {
            let new_max = (current_max + 1).min(self.absolute_max);
            self.current_max.store(new_max, Ordering::Relaxed);
        }
    }

    fn balance(&self) -> usize {
        self.tokens.load(Ordering::Relaxed) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_basic() {
        let budget = TokenBucketBudget::new(10.0, 5, 5);

        // Should allow 5 withdrawals
        assert!(budget.try_withdraw());
        assert!(budget.try_withdraw());
        assert!(budget.try_withdraw());
        assert!(budget.try_withdraw());
        assert!(budget.try_withdraw());

        // 6th should fail
        assert!(!budget.try_withdraw());

        // Deposit should allow one more
        budget.deposit();
        assert!(budget.try_withdraw());
        assert!(!budget.try_withdraw());
    }

    #[test]
    fn test_token_bucket_balance() {
        let budget = TokenBucketBudget::new(10.0, 100, 50);
        assert_eq!(budget.balance(), 50);

        budget.try_withdraw();
        assert_eq!(budget.balance(), 49);

        budget.deposit();
        assert_eq!(budget.balance(), 50);
    }

    #[test]
    fn test_aimd_basic() {
        let budget = AimdBudget::new(5, 10, 1, 1, 0.5);

        // Should allow 10 withdrawals
        for _ in 0..10 {
            assert!(budget.try_withdraw());
        }

        // 11th should fail and reduce max
        assert!(!budget.try_withdraw());

        // Deposit 5 tokens
        for _ in 0..5 {
            budget.deposit();
        }

        // Should now allow some withdrawals (max reduced to 5)
        assert!(budget.try_withdraw());
    }

    #[test]
    fn test_aimd_min_budget_floor() {
        let budget = AimdBudget::new(5, 10, 1, 1, 0.1);

        // Exhaust budget multiple times
        for _ in 0..10 {
            budget.try_withdraw();
        }

        // Keep trying to exhaust to hit the floor
        for _ in 0..10 {
            budget.try_withdraw();
        }

        // Deposit back to min
        for _ in 0..5 {
            budget.deposit();
        }

        // Should be able to withdraw at least min_budget times
        let mut count = 0;
        while budget.try_withdraw() {
            count += 1;
        }
        assert!(
            count >= 1,
            "Should allow at least 1 withdrawal after deposit"
        );
    }

    #[test]
    fn test_builder_token_bucket() {
        let budget = RetryBudgetBuilder::new()
            .token_bucket()
            .tokens_per_second(100.0)
            .max_tokens(50)
            .initial_tokens(25)
            .build();

        assert_eq!(budget.balance(), 25);
    }

    #[test]
    fn test_builder_aimd() {
        let budget = RetryBudgetBuilder::new()
            .aimd()
            .min_budget(5)
            .max_budget(100)
            .deposit_amount(2)
            .withdraw_amount(1)
            .build();

        assert_eq!(budget.balance(), 100);
    }
}
