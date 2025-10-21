# Add dedicated regression tests for metrics output

## Context
Workspace tests enable the `metrics` feature across crates, yet coverage primarily asserts circuit breaker counters. Other patterns (retry, rate limiter, cache, bulkhead, chaos, time limiter) lack targeted checks for emitted metric names, units, and labels, leaving instrumentation drift undetected.

## Proposal
- Introduce shared helpers (e.g., a debug recorder harness) to capture metrics across patterns with the feature enabled.
- Add focused integration tests validating that each pattern exports the expected counters/histograms with stable naming conventions and key labels (`outcome`, `state`, etc.).
- Consider gating the suite behind a feature flag if runtime cost is high, but ensure CI executes it regularly.

## Acceptance Criteria
- Tests fail when metric names or critical labels change unintentionally.
- Coverage spans every pattern that declares the `metrics` feature.
- Contributor documentation references the new suite so future changes update metrics expectations alongside code.
