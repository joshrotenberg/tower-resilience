# Surface panic details in EventListeners::emit

## Context
`EventListeners::emit` in `crates/tower-resilience-core/src/events.rs` wraps each listener in `catch_unwind` to preserve resiliency, but any panic is silently discarded. Teams wiring observability hooks currently lose visibility into misbehaving listeners, which complicates debugging and can hide broken integrations.

## Proposal
- Emit a tracing warning (or metrics counter) when a listener panics so operators can discover failures without crashing the pipeline.
- Make the diagnostic lightweight and optional: respect existing `tracing`/`metrics` feature flags and avoid allocating unless a panic occurs.
- Document the behavior in module docs so downstream implementers know that panics are caught yet reported.

## Acceptance Criteria
- Panicking listeners trigger an observable warning or counter while allowing other listeners to run.
- Behavior covered by unit tests that assert logging/metrics integration when the relevant feature is enabled.
- Documentation updated to explain the panic-handling and observability strategy.
