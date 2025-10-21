# Implement tower::Layer for CircuitBreakerLayer

## Context
`CircuitBreakerLayer` currently exposes a manual `.layer(service)` helper instead of implementing `tower::Layer`. This forces callers to wrap services manually and breaks composability with `ServiceBuilder`, even though the layer only needs the request type at call time.

## Proposal
- Explore a blanket `impl<S, Req>` for `tower::Layer` where `S: Service<Req>` and the response/error types match the layer configuration.
- Ensure the implementation preserves the existing builder ergonomics and does not regress type inference for async closures.
- Provide doctests or examples showing the layer used directly in `ServiceBuilder::layer`.

## Acceptance Criteria
- `CircuitBreakerLayer` implements `tower::Layer` behind the existing feature gates, compiling for common service signatures.
- New unit or integration coverage demonstrates stacking the layer with `ServiceBuilder`.
- Documentation updated (README or crate docs) to show the improved ergonomic usage.
