# Tower service review checklist

Paste this checklist into a pull request that adds or materially changes a
`tower::Service` implementation. Mark an item `N/A` only with a short reason;
middleware policies differ, but the base Tower laws do not.

This is the short operational companion to the project
[contract matrix](tower-contract-matrix.md). The rationale and longer-term
verification options are captured in the
[Tower service contract verification research note](https://gist.github.com/joshrotenberg/017d09e16dd7a9592628f5206062444b).

## Base readiness laws

- [ ] Every `call` follows `Poll::Ready(Ok(()))` from `poll_ready` on the same
      logical service instance.
- [ ] A successful readiness grant remains usable until one call consumes it
      or a readiness error makes the instance terminal.
- [ ] `Pending` stores a usable waker and the state transition that may enable
      progress wakes it; there is no unconditional self-wake loop.
- [ ] `Ready(Err(_))` is treated as terminal for that instance and is preserved
      or mapped deliberately.
- [ ] `call` does not manufacture a task context or drive `poll_ready` itself.

## Ownership, cloning, and admission

- [ ] The implementation calls the exact inner value that was made ready; a
      fresh clone does not accidentally inherit an instance-local grant.
- [ ] Clone semantics are explicit: readiness, permits, counters, and routing
      progression are intentionally shared or intentionally independent.
- [ ] Capacity is reserved atomically before admission and cannot be spent by
      multiple concurrent clones.
- [ ] Repeated `poll_ready` calls do not double-acquire, replace, or lose a
      reservation.

## Drop and cancellation

- [ ] Dropping the service after readiness but before `call` releases any
      reservation.
- [ ] Dropping the response future releases capacity and other owned resources
      on success, error, and pending paths.
- [ ] Spawned or external work has an explicit lifecycle; dropping a handle
      does not silently detach work when cancellation is promised.
- [ ] Racing/fan-out middleware cancels and accounts for losing attempts before
      the winning future returns.

## Errors, values, and policy

- [ ] Responses and call errors are preserved without an undocumented lossy
      conversion.
- [ ] Readiness and call errors are classified and observed consistently.
- [ ] Any intentional buffering, shedding, backpressure, or always-ready policy
      explains how it remains valid for a readiness-sensitive inner service.
- [ ] Invalid, zero, non-finite, and inverted-bound configuration is rejected
      explicitly rather than panicking or busy-spinning.

## Evidence

- [ ] `ServiceProbe`/`ControlledService` exercises readiness, clones, wakeups,
      peak in-flight work, and dropped futures without wall-clock sleeps.
- [ ] A deliberately nonconforming fixture fails with a specific contract
      assertion.
- [ ] A Tower differential scenario exists where core Tower provides comparable
      middleware or a useful behavioral reference.
- [ ] Contended shared-state code has deterministic, property, Loom, Shuttle,
      or equivalent schedule coverage proportional to its risk.
- [ ] Effect-specific cleanup (tasks, sockets, subprocesses, remote requests)
      is tested separately from merely observing response-future drop.
