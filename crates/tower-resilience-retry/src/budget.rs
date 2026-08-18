//! Retry budget implementations to prevent retry storms.
//!
//! Retry budgets limit the total number of retries across all requests,
//! preventing cascading failures when a downstream service is struggling.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_resilience_core::aimd::{AimdConfig, AimdConfigError, AimdController};

/// Errors that can occur when constructing a retry budget.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum RetryBudgetConfigError {
    /// `tokens_per_second` is not finite, or is negative.
    #[error("tokens_per_second ({0}) must be finite and non-negative")]
    InvalidTokensPerSecond(f64),
    /// `initial_tokens` exceeds `max_tokens`.
    #[error("initial_tokens ({initial_tokens}) must not exceed max_tokens ({max_tokens})")]
    InitialExceedsMax {
        /// The configured initial token count.
        initial_tokens: usize,
        /// The configured maximum token count.
        max_tokens: usize,
    },
    /// `deposit_amount` is zero, which would make deposits a no-op.
    #[error("deposit_amount must be greater than zero")]
    ZeroDepositAmount,
    /// `withdraw_amount` is zero, which would make withdrawals free.
    #[error("withdraw_amount must be greater than zero")]
    ZeroWithdrawAmount,
    /// The underlying AIMD controller configuration is invalid.
    #[error(transparent)]
    Aimd(#[from] AimdConfigError),
}

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
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let budget = RetryBudgetBuilder::new()
    ///     .token_bucket()
    ///     .tokens_per_second(10.0)
    ///     .max_tokens(100)
    ///     .build()?;
    /// # Ok(())
    /// # }
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
    /// This uses the shared AIMD controller from `tower-resilience-core`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_retry::RetryBudgetBuilder;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let budget = RetryBudgetBuilder::new()
    ///     .aimd()
    ///     .min_budget(10)
    ///     .max_budget(1000)
    ///     .build()?;
    /// # Ok(())
    /// # }
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
    ///
    /// # Errors
    ///
    /// Returns [`RetryBudgetConfigError::InvalidTokensPerSecond`] if the rate is not
    /// finite or is negative, or [`RetryBudgetConfigError::InitialExceedsMax`] if
    /// `initial_tokens` exceeds `max_tokens`.
    pub fn build(self) -> Result<Arc<dyn RetryBudget>, RetryBudgetConfigError> {
        Ok(Arc::new(TokenBucketBudget::new(
            self.tokens_per_second,
            self.max_tokens,
            self.initial_tokens.unwrap_or(self.max_tokens),
        )?))
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
    ///
    /// # Errors
    ///
    /// Returns [`RetryBudgetConfigError::ZeroDepositAmount`] or
    /// [`RetryBudgetConfigError::ZeroWithdrawAmount`] if either amount is zero, or
    /// [`RetryBudgetConfigError::Aimd`] if the derived AIMD controller configuration
    /// is invalid (for example, `min_budget` exceeds `max_budget`).
    pub fn build(self) -> Result<Arc<dyn RetryBudget>, RetryBudgetConfigError> {
        Ok(Arc::new(AimdBudget::new(
            self.min_budget,
            self.max_budget,
            self.deposit_amount,
            self.withdraw_amount,
            self.decrease_factor,
        )?))
    }
}

/// A source of elapsed time, used to drive time-based token refill.
///
/// Production budgets use [`MonotonicClock`]. Tests inject a manual clock so
/// refill behavior can be verified deterministically without real sleeps.
trait BudgetClock: Send + Sync {
    /// Elapsed time since the clock was created.
    fn now(&self) -> Duration;
}

/// [`BudgetClock`] implementation backed by [`Instant`].
struct MonotonicClock {
    start: Instant,
}

impl MonotonicClock {
    fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl BudgetClock for MonotonicClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Mutable, mutex-guarded state for a [`TokenBucketBudget`].
///
/// All fields are read, refilled, and mutated under a single lock acquisition
/// so concurrent `try_withdraw`/`deposit`/`balance` calls cannot race.
struct TokenBucketState {
    /// Whole tokens currently available.
    tokens: usize,
    /// Fractional token credit carried between refills, always in `[0.0, 1.0)`.
    fractional: f64,
    /// Elapsed time (per the budget's clock) at the last refill.
    last_refill: Duration,
}

impl TokenBucketState {
    /// Lazily apply time-based refill up to `now`, capped at `max_tokens`.
    ///
    /// Fractional token credit is preserved across calls (and across the cap)
    /// so a slow rate like 0.5/sec still eventually yields whole tokens.
    fn refill(&mut self, now: Duration, tokens_per_second: f64, max_tokens: usize) {
        if tokens_per_second > 0.0 {
            let elapsed = now.saturating_sub(self.last_refill);
            if elapsed > Duration::ZERO {
                let gained = self.fractional + elapsed.as_secs_f64() * tokens_per_second;
                let whole_gained = gained.floor();
                self.fractional = gained - whole_gained;
                let capacity_left = max_tokens.saturating_sub(self.tokens) as f64;
                let whole_gained = whole_gained.min(capacity_left).max(0.0) as usize;
                self.tokens = self.tokens.saturating_add(whole_gained).min(max_tokens);
            }
        }
        self.last_refill = now;
    }
}

/// Token bucket retry budget.
///
/// Tokens are replenished continuously at `tokens_per_second` (elapsed-time
/// based refill, computed lazily on each access) and are additionally
/// deposited by successful requests. Tokens are consumed by retries. This
/// provides a simple way to limit retry storms.
///
/// This budget is intentionally comparable to Tower's own transactions-per-second
/// (TPS) retry budget concept; see the upstream tracking issues
/// [tower-rs/tower#857](https://github.com/tower-rs/tower/issues/857) and
/// [tower-rs/tower#863](https://github.com/tower-rs/tower/issues/863) for
/// discussion of TPS budget semantics parity.
pub struct TokenBucketBudget {
    /// Token balance and refill bookkeeping, serialized under one lock.
    state: Mutex<TokenBucketState>,
    /// Token refill rate, in tokens per second. `0.0` disables time-based refill.
    tokens_per_second: f64,
    /// Maximum tokens (burst capacity).
    max_tokens: usize,
    /// Source of elapsed time driving refill.
    clock: Arc<dyn BudgetClock>,
}

impl std::fmt::Debug for TokenBucketBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        f.debug_struct("TokenBucketBudget")
            .field("tokens", &state.tokens)
            .field("fractional", &state.fractional)
            .field("tokens_per_second", &self.tokens_per_second)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl TokenBucketBudget {
    /// Create a new token bucket budget.
    ///
    /// # Errors
    ///
    /// Returns [`RetryBudgetConfigError::InvalidTokensPerSecond`] if `tokens_per_second`
    /// is not finite or is negative, or [`RetryBudgetConfigError::InitialExceedsMax`]
    /// if `initial_tokens` exceeds `max_tokens`.
    pub fn new(
        tokens_per_second: f64,
        max_tokens: usize,
        initial_tokens: usize,
    ) -> Result<Self, RetryBudgetConfigError> {
        Self::with_clock(
            tokens_per_second,
            max_tokens,
            initial_tokens,
            Arc::new(MonotonicClock::new()),
        )
    }

    /// Test-only constructor that injects a manual clock instead of wall-clock time.
    #[cfg(test)]
    fn new_with_clock(
        tokens_per_second: f64,
        max_tokens: usize,
        initial_tokens: usize,
        clock: Arc<dyn BudgetClock>,
    ) -> Result<Self, RetryBudgetConfigError> {
        Self::with_clock(tokens_per_second, max_tokens, initial_tokens, clock)
    }

    fn with_clock(
        tokens_per_second: f64,
        max_tokens: usize,
        initial_tokens: usize,
        clock: Arc<dyn BudgetClock>,
    ) -> Result<Self, RetryBudgetConfigError> {
        if !tokens_per_second.is_finite() || tokens_per_second < 0.0 {
            return Err(RetryBudgetConfigError::InvalidTokensPerSecond(
                tokens_per_second,
            ));
        }
        if initial_tokens > max_tokens {
            return Err(RetryBudgetConfigError::InitialExceedsMax {
                initial_tokens,
                max_tokens,
            });
        }

        let now = clock.now();
        Ok(Self {
            state: Mutex::new(TokenBucketState {
                tokens: initial_tokens,
                fractional: 0.0,
                last_refill: now,
            }),
            tokens_per_second,
            max_tokens,
            clock,
        })
    }
}

impl RetryBudget for TokenBucketBudget {
    fn try_withdraw(&self) -> bool {
        let now = self.clock.now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.refill(now, self.tokens_per_second, self.max_tokens);

        if state.tokens >= 1 {
            state.tokens -= 1;
            true
        } else {
            false
        }
    }

    fn deposit(&self) {
        let now = self.clock.now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.refill(now, self.tokens_per_second, self.max_tokens);
        state.tokens = (state.tokens + 1).min(self.max_tokens);
    }

    fn balance(&self) -> usize {
        let now = self.clock.now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.refill(now, self.tokens_per_second, self.max_tokens);
        state.tokens
    }
}

/// AIMD (Additive Increase Multiplicative Decrease) retry budget.
///
/// The budget grows linearly with successful requests and shrinks
/// multiplicatively when retries are rejected.
///
/// This implementation uses the shared [`AimdController`] from `tower-resilience-core`
/// to manage the dynamic maximum limit, while tracking the current token balance
/// separately under its own lock.
#[derive(Debug)]
pub struct AimdBudget {
    /// Current token balance, serialized under one lock.
    tokens: Mutex<usize>,
    /// AIMD controller for the maximum budget limit.
    limit_controller: AimdController,
    /// Tokens to add on deposit.
    deposit_amount: usize,
    /// Tokens to remove on withdraw.
    withdraw_amount: usize,
}

impl AimdBudget {
    /// Create a new AIMD budget.
    ///
    /// # Errors
    ///
    /// Returns [`RetryBudgetConfigError::ZeroDepositAmount`] if `deposit_amount` is
    /// zero, [`RetryBudgetConfigError::ZeroWithdrawAmount`] if `withdraw_amount` is
    /// zero, or [`RetryBudgetConfigError::Aimd`] if the derived [`AimdConfig`] is
    /// invalid (for example, `min_budget` exceeds `max_budget`).
    pub fn new(
        min_budget: usize,
        max_budget: usize,
        deposit_amount: usize,
        withdraw_amount: usize,
        decrease_factor: f64,
    ) -> Result<Self, RetryBudgetConfigError> {
        if deposit_amount == 0 {
            return Err(RetryBudgetConfigError::ZeroDepositAmount);
        }
        if withdraw_amount == 0 {
            return Err(RetryBudgetConfigError::ZeroWithdrawAmount);
        }

        let config = AimdConfig::new()
            .with_initial_limit(max_budget)
            .with_min_limit(min_budget)
            .with_max_limit(max_budget)
            .with_increase_by(1) // Slowly recover the max limit
            .with_decrease_factor(decrease_factor);

        let limit_controller = AimdController::new(config)?;

        Ok(Self {
            tokens: Mutex::new(max_budget),
            limit_controller,
            deposit_amount,
            withdraw_amount,
        })
    }

    /// Get the current maximum limit (controlled by AIMD).
    pub fn current_max(&self) -> usize {
        self.limit_controller.limit()
    }
}

impl RetryBudget for AimdBudget {
    fn try_withdraw(&self) -> bool {
        let mut tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());

        if *tokens >= self.withdraw_amount {
            *tokens -= self.withdraw_amount;
            true
        } else {
            // Budget exhausted - apply multiplicative decrease to max via controller,
            // then clamp the balance to the (possibly lower) new max.
            self.limit_controller.record_failure();
            let new_max = self.limit_controller.limit();
            if *tokens > new_max {
                *tokens = new_max;
            }
            false
        }
    }

    fn deposit(&self) {
        let mut tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());

        // Slowly increase the max back toward the absolute max via the controller.
        self.limit_controller.record_success();
        let current_max = self.limit_controller.limit();

        // Additive increase: add deposit amount, cap at current max.
        *tokens = (*tokens + self.deposit_amount).min(current_max);
    }

    fn balance(&self) -> usize {
        *self.tokens.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    /// Manual [`BudgetClock`] for deterministic, virtual-time tests.
    struct ManualClock {
        now: Mutex<Duration>,
    }

    impl ManualClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(Duration::ZERO),
            })
        }

        fn advance(&self, delta: Duration) {
            let mut now = self.now.lock().unwrap();
            *now += delta;
        }
    }

    impl BudgetClock for ManualClock {
        fn now(&self) -> Duration {
            *self.now.lock().unwrap()
        }
    }

    // --- Basic behavior (virtual time, no drift) -----------------------

    #[test]
    fn test_token_bucket_initial_burst_and_success_deposit() {
        let clock = ManualClock::new();
        let budget = TokenBucketBudget::new_with_clock(0.0, 5, 5, clock).unwrap();

        // Should allow 5 withdrawals (initial burst).
        for _ in 0..5 {
            assert!(budget.try_withdraw());
        }

        // 6th should fail - budget exhausted, no time-based refill (rate is 0).
        assert!(!budget.try_withdraw());

        // A success deposit allows exactly one more withdrawal.
        budget.deposit();
        assert!(budget.try_withdraw());
        assert!(!budget.try_withdraw());
    }

    #[test]
    fn test_token_bucket_balance() {
        let clock = ManualClock::new();
        let budget = TokenBucketBudget::new_with_clock(0.0, 100, 50, clock).unwrap();
        assert_eq!(budget.balance(), 50);

        budget.try_withdraw();
        assert_eq!(budget.balance(), 49);

        budget.deposit();
        assert_eq!(budget.balance(), 50);
    }

    #[test]
    fn test_token_bucket_zero_rate_disables_time_refill_but_allows_deposit() {
        let clock = ManualClock::new();
        let budget = TokenBucketBudget::new_with_clock(0.0, 10, 0, clock.clone()).unwrap();
        assert_eq!(budget.balance(), 0);

        // Elapsed time alone must not replenish when the rate is zero.
        clock.advance(Duration::from_secs(3600));
        assert_eq!(budget.balance(), 0);

        // Deposits still work.
        budget.deposit();
        assert_eq!(budget.balance(), 1);
    }

    #[test]
    fn test_token_bucket_fractional_rate_refill_accrues_over_several_advances() {
        let clock = ManualClock::new();
        // 0.5 tokens/sec: every 2 seconds of elapsed time yields exactly 1 token.
        let budget = TokenBucketBudget::new_with_clock(0.5, 10, 0, clock.clone()).unwrap();
        assert_eq!(budget.balance(), 0);

        clock.advance(Duration::from_secs(2));
        assert_eq!(budget.balance(), 1);

        clock.advance(Duration::from_secs(2));
        assert_eq!(budget.balance(), 2);

        // Two 1-second advances (each sub-token on their own) sum to one more
        // whole token, demonstrating the fractional carry is retained across
        // separate calls rather than being reset or truncated away.
        clock.advance(Duration::from_secs(1));
        assert_eq!(budget.balance(), 2);
        clock.advance(Duration::from_secs(1));
        assert_eq!(budget.balance(), 3);

        // A very large elapsed time caps at max_tokens rather than overflowing.
        clock.advance(Duration::from_secs(1000));
        assert_eq!(budget.balance(), 10);
    }

    #[test]
    fn test_token_bucket_success_deposit_preserves_fractional_credit() {
        let clock = ManualClock::new();
        let budget = TokenBucketBudget::new_with_clock(0.5, 10, 0, clock.clone()).unwrap();

        // Accrue half a token (rate 0.5/sec over 1 second).
        clock.advance(Duration::from_secs(1));
        assert_eq!(budget.balance(), 0);

        // A success deposit adds one whole token on top of time-based refill,
        // without discarding the pending fractional credit.
        budget.deposit();
        assert_eq!(budget.balance(), 1);

        // The other half of the original second's credit is still pending;
        // one more second completes it into a second whole token.
        clock.advance(Duration::from_secs(1));
        assert_eq!(budget.balance(), 2);
    }

    // --- AIMD basic behavior ---------------------------------------------

    #[test]
    fn test_aimd_basic() {
        let budget = AimdBudget::new(5, 10, 1, 1, 0.5).unwrap();

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
        let budget = AimdBudget::new(5, 10, 1, 1, 0.1).unwrap();

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
    fn test_aimd_current_max() {
        let budget = AimdBudget::new(5, 100, 1, 1, 0.5).unwrap();
        assert_eq!(budget.current_max(), 100);

        // Exhaust and trigger decrease
        for _ in 0..100 {
            budget.try_withdraw();
        }
        budget.try_withdraw(); // This triggers the decrease

        assert_eq!(budget.current_max(), 50);
    }

    #[test]
    fn test_aimd_uses_shared_controller() {
        // Verify that the AIMD budget correctly uses the shared controller
        let budget = AimdBudget::new(1, 10, 1, 1, 0.5).unwrap();

        // Exhaust tokens
        for _ in 0..10 {
            budget.try_withdraw();
        }

        // Trigger multiple failures to see multiplicative decrease
        budget.try_withdraw(); // max -> 5
        assert_eq!(budget.current_max(), 5);

        budget.try_withdraw(); // max -> 2
        assert_eq!(budget.current_max(), 2);

        budget.try_withdraw(); // max -> 1 (min)
        assert_eq!(budget.current_max(), 1);
    }

    #[test]
    fn test_builder_token_bucket() {
        let budget = RetryBudgetBuilder::new()
            .token_bucket()
            .tokens_per_second(100.0)
            .max_tokens(50)
            .initial_tokens(25)
            .build()
            .unwrap();

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
            .build()
            .unwrap();

        assert_eq!(budget.balance(), 100);
    }

    // --- Configuration errors --------------------------------------------

    #[test]
    fn test_token_bucket_rejects_nan_rate() {
        let err = TokenBucketBudget::new(f64::NAN, 10, 5).unwrap_err();
        match err {
            RetryBudgetConfigError::InvalidTokensPerSecond(rate) => assert!(rate.is_nan()),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_token_bucket_rejects_negative_rate() {
        let err = TokenBucketBudget::new(-1.0, 10, 5).unwrap_err();
        assert_eq!(err, RetryBudgetConfigError::InvalidTokensPerSecond(-1.0));
    }

    #[test]
    fn test_token_bucket_rejects_initial_exceeding_max() {
        let err = TokenBucketBudget::new(1.0, 5, 10).unwrap_err();
        assert_eq!(
            err,
            RetryBudgetConfigError::InitialExceedsMax {
                initial_tokens: 10,
                max_tokens: 5,
            }
        );
    }

    #[test]
    fn test_aimd_rejects_zero_deposit_amount() {
        let err = AimdBudget::new(1, 10, 0, 1, 0.5).unwrap_err();
        assert_eq!(err, RetryBudgetConfigError::ZeroDepositAmount);
    }

    #[test]
    fn test_aimd_rejects_zero_withdraw_amount() {
        let err = AimdBudget::new(1, 10, 1, 0, 0.5).unwrap_err();
        assert_eq!(err, RetryBudgetConfigError::ZeroWithdrawAmount);
    }

    #[test]
    fn test_aimd_propagates_min_exceeds_max_config_error() {
        let err = AimdBudget::new(50, 10, 1, 1, 0.5).unwrap_err();
        assert_eq!(
            err,
            RetryBudgetConfigError::Aimd(AimdConfigError::MinExceedsMax {
                min_limit: 50,
                max_limit: 10,
            })
        );
    }

    // --- Contention (real threads, no manual clock) ----------------------

    #[test]
    fn test_token_bucket_concurrent_deposits_are_not_lost() {
        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 1000;

        let budget = Arc::new(TokenBucketBudget::new(0.0, 100_000, 0).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));

        thread::scope(|s| {
            for _ in 0..THREADS {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                s.spawn(move || {
                    barrier.wait();
                    for _ in 0..CALLS_PER_THREAD {
                        budget.deposit();
                    }
                });
            }
        });

        assert_eq!(budget.balance(), THREADS * CALLS_PER_THREAD);
    }

    #[test]
    fn test_token_bucket_concurrent_withdrawals_never_exceed_available_balance() {
        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 200;
        const INITIAL_TOKENS: usize = 1000;

        let budget = Arc::new(TokenBucketBudget::new(0.0, INITIAL_TOKENS, INITIAL_TOKENS).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let successes = Arc::new(AtomicUsize::new(0));

        thread::scope(|s| {
            for _ in 0..THREADS {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                let successes = Arc::clone(&successes);
                s.spawn(move || {
                    barrier.wait();
                    for _ in 0..CALLS_PER_THREAD {
                        if budget.try_withdraw() {
                            successes.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                });
            }
        });

        // THREADS * CALLS_PER_THREAD attempts exceed INITIAL_TOKENS, so exactly
        // INITIAL_TOKENS withdrawals must succeed - never more, never fewer.
        assert_eq!(successes.load(Ordering::SeqCst), INITIAL_TOKENS);
        assert_eq!(budget.balance(), 0);
    }

    #[test]
    fn test_token_bucket_concurrent_mixed_deposit_withdraw_nets_out() {
        const DEPOSIT_THREADS: usize = 8;
        const DEPOSITS_PER_THREAD: usize = 500;
        const WITHDRAW_THREADS: usize = 8;
        const WITHDRAW_ATTEMPTS_PER_THREAD: usize = 100;
        const INITIAL_TOKENS: usize = 500;

        let budget = Arc::new(TokenBucketBudget::new(0.0, 1_000_000, INITIAL_TOKENS).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(DEPOSIT_THREADS + WITHDRAW_THREADS));
        let successful_withdrawals = Arc::new(AtomicUsize::new(0));

        thread::scope(|s| {
            for _ in 0..DEPOSIT_THREADS {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                s.spawn(move || {
                    barrier.wait();
                    for _ in 0..DEPOSITS_PER_THREAD {
                        budget.deposit();
                    }
                });
            }

            for _ in 0..WITHDRAW_THREADS {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                let successful_withdrawals = Arc::clone(&successful_withdrawals);
                s.spawn(move || {
                    barrier.wait();
                    for _ in 0..WITHDRAW_ATTEMPTS_PER_THREAD {
                        if budget.try_withdraw() {
                            successful_withdrawals.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                });
            }
        });

        let total_deposits = DEPOSIT_THREADS * DEPOSITS_PER_THREAD;
        let expected =
            INITIAL_TOKENS + total_deposits - successful_withdrawals.load(Ordering::SeqCst);
        assert_eq!(budget.balance(), expected);
    }

    #[test]
    fn test_aimd_concurrent_deposits_are_not_lost_and_stay_within_current_max() {
        const THREADS: usize = 16;
        const CALLS_PER_THREAD: usize = 300;
        const MAX_BUDGET: usize = 20_000;
        const INITIAL_WITHDRAWALS: usize = 5_000;

        let budget = Arc::new(AimdBudget::new(1, MAX_BUDGET, 1, 1, 0.5).unwrap());

        // Drain some tokens serially, without exhausting the budget, so the
        // AIMD max stays at MAX_BUDGET (no record_failure triggered) while
        // leaving headroom for the concurrent deposits below.
        for _ in 0..INITIAL_WITHDRAWALS {
            assert!(budget.try_withdraw());
        }
        assert_eq!(budget.current_max(), MAX_BUDGET);

        let barrier = Arc::new(std::sync::Barrier::new(THREADS));

        thread::scope(|s| {
            for _ in 0..THREADS {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                s.spawn(move || {
                    barrier.wait();
                    for _ in 0..CALLS_PER_THREAD {
                        budget.deposit();
                    }
                });
            }
        });

        let expected = MAX_BUDGET - INITIAL_WITHDRAWALS + THREADS * CALLS_PER_THREAD;
        assert_eq!(budget.current_max(), MAX_BUDGET);
        assert_eq!(budget.balance(), expected);
        assert!(budget.balance() <= budget.current_max());
    }
}
