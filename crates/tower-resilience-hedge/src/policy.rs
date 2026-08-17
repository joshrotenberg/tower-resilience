//! Per-request eligibility policies for hedged execution.

use std::fmt;
use std::sync::Arc;

/// Decides whether a request may be executed concurrently as a hedge.
///
/// Implementations must return `true` only when sending more than one copy of
/// the request is safe. In most applications that means the operation is
/// idempotent, or that it carries an idempotency key understood by the
/// downstream service.
pub trait HedgePolicy<Req>: Send + Sync + 'static {
    /// Returns `true` when `request` is eligible for hedging.
    fn is_eligible(&self, request: &Req) -> bool;
}

/// Policy that treats every request as eligible for hedging.
///
/// This is the default for backward compatibility. It is appropriate only
/// when every request accepted by the wrapped service is safe to execute more
/// than once concurrently. Mixed read/write request types should configure
/// [`HedgeConfigBuilder::eligible_if`](crate::HedgeConfigBuilder::eligible_if)
/// instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlwaysHedge;

impl<Req> HedgePolicy<Req> for AlwaysHedge {
    fn is_eligible(&self, _request: &Req) -> bool {
        true
    }
}

/// A typed predicate used to decide request eligibility.
#[derive(Clone)]
pub struct HedgePredicate<Req> {
    predicate: Arc<dyn Fn(&Req) -> bool + Send + Sync>,
}

impl<Req> HedgePredicate<Req> {
    /// Creates an eligibility policy from a request predicate.
    pub fn new<F>(predicate: F) -> Self
    where
        F: Fn(&Req) -> bool + Send + Sync + 'static,
    {
        Self {
            predicate: Arc::new(predicate),
        }
    }
}

impl<Req> fmt::Debug for HedgePredicate<Req> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HedgePredicate")
            .finish_non_exhaustive()
    }
}

impl<Req: 'static> HedgePolicy<Req> for HedgePredicate<Req> {
    fn is_eligible(&self, request: &Req) -> bool {
        (self.predicate)(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_policy_accepts_every_request() {
        assert!(AlwaysHedge.is_eligible(&"write"));
    }

    #[test]
    fn predicate_classifies_requests() {
        let policy = HedgePredicate::new(|request: &&str| *request == "read");

        assert!(policy.is_eligible(&"read"));
        assert!(!policy.is_eligible(&"write"));
    }
}
