# Tower contract test matrix

This matrix is the source of truth for `tower::Service` contract coverage. A
green baseline means a layer drives the exact readied receiver and composes
with Tower's stateful `ConcurrencyLimit`. The adversarial columns use
`tower_resilience_core::testing::{ServiceProbe, ControlledService}` so tests can
observe internal attempts, cancellation, in-flight admission, wakeups, and
value preservation deterministically.

Use the companion [Tower service review checklist](tower-service-review-checklist.md)
for the human review pass on each implementation.

`Gap` is intentional and links to the ordered issue that must turn the cell
into a regression test. It does **not** mean the behavior is assumed correct.

| Crate | Readied receiver / Tower composition | Internal attempts | Cancellation / drop | Admission / wake | Response / error preservation | Configuration edges | Differential reference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| adaptive | Covered in crate + umbrella contract tests | N/A | Covered for readied-clone and call-future drop ([#365]) | Covered for clone-heavy reservation, wake, and safe dynamic shrink ([#365]); atomic controller updates remain ([#368]) | Covered by contract and behavior tests | Gap ([#368], [#372]) | `tower::limit::ConcurrencyLimit` |
| bulkhead | Covered in rejection and backpressure modes | N/A | Waiting clone, readied clone, and response-future drop release queue position/permits ([#367]) | Direct `PollSemaphore` reservation matches Tower; fail-fast and bounded-wait modes are additional ([#367]) | Inner readiness and closed-semaphore errors release reservations | Audit in [#372] | `tower::limit::ConcurrencyLimit` |
| cache | Covered on forced misses | N/A | Pass-through cancellation; probe regression not yet added | Covered by existing concurrency tests | Covered on hits/misses | Audit in [#372] | `tower::Service` pass-through |
| chaos | Covered | N/A | Pass-through cancellation; probe regression not yet added | N/A | Covered by behavior tests | Audit in [#372] | `tower::Service` pass-through |
| circuitbreaker | Covered, including state-first open-circuit gating ([#381]) | N/A | Half-open probe drop returns its reservation and records no result ([#382]); permanently-pending inner under `Closed` forwards `Pending`, registers a waker, and is drop-safe, with no busy-poll ([#375]) | Half-open admission is reserved with a per-cycle `Semaphore` shared across clones; open rejection/backpressure gate covered ([#381], [#382]) | Inner readiness errors covered | Failure-rate window fill/trip, `ConsecutiveFailures` model (independent of window gating), and slow-call-rate trip are each covered as distinct paths ([#375]) | Tower circuit breaker [#855]; comparison and RMQTT/GovCraft integration review in [circuitbreaker-tower-comparison.md] |
| coalesce | Covered on leader path | N/A | Leader/waiter cancellation has behavior coverage; probe regression not yet added | Covered by concurrency/stress tests | Covered for leaders/waiters | Audit in [#372] | Singleflight semantics |
| executor | Covered | N/A | Executor cancellation semantics tracked in [#371] | N/A | Covered by contract/behavior tests | Audit in [#372] | `tower::Service` pass-through |
| fallback | Covered for primary and generic backup services ([#374]) | Backup readiness is driven after primary failure on the exact shared service instance ([#374]) | Pending readiness and backup-call cancellation release shared state and drop owned work ([#374]) | Backup readiness registers its service waker; readiness errors are typed fallback failures ([#374]) | Primary skip and heterogeneous backup-error selection covered ([#374]) | Request cloning and shared-backup topology documented; broader audit in [#372] | Tower [#413] service-to-service fallback; [#870] is pre-primary interception |
| hedge | Covered for delayed and parallel attempts | Covered for attempt readiness and eligibility gating | Covered for losing attempts and caller-future drop ([#369]) | Covered for ineligible single-attempt admission ([#369]) | Covered for first success and primary-error selection | Eligibility and minimum-attempt edges covered; broader audit in [#372] | `tower::Service` cancellation contract |
| outlier | Covered | N/A | Pass-through cancellation; probe regression not yet added | Covered by detector tests | Covered by behavior tests | Audit in [#372] | `tower::load::Load` routing semantics |
| ratelimiter | Covered in rejection and backpressure modes | N/A | Waiter drop is cancellation-safe ([#363]) | Barrier-driven clone contention consumes exactly one permit per admission ([#363]) | Permit/rejection results covered across all algorithms | Zero/overflow validation covered; broader audit in [#372] | `tower::limit::RateLimit` |
| reconnect | Covered for factory, connected service, and retried request | Covered with monotonically increasing service-instance IDs ([#361]) | Shared reconnect survives caller drop; explicit cancellation regression remains | Clone failures coalesce onto one replacement generation ([#361]) | Factory and service errors remain typed | Factory target and retry policy covered; broader audit in [#372] | `tower::reconnect::Reconnect` / `MakeService` |
| retry | Covered for initial and internal attempts ([#362]) | Covered: fresh readiness before every retry ([#362]) | Pass-through cancellation; dedicated retry-wait drop coverage not yet added | Internal readiness errors are terminal and preserved ([#362]); pending-wake coverage remains | Covered by policy and readiness-error tests | Audit in [#372] | `tower::retry::Retry` |
| router | Covered across two backends and clone-per-request traffic | N/A | Pass-through cancellation; dedicated probe regression not yet added | All-backend readiness is intentional and documented; unlike `Balance`, one pending backend gates the router | Covered by routing and event tests | Empty/zero and maximum-weight arithmetic covered ([#366]); broader audit in [#372] | `tower::steer::Steer` fixed-set readiness / `tower::balance::p2c::Balance` dynamic load-aware routing |
| timelimiter | Covered | N/A | Covered by cancellation tests | Readiness/deadline scope tracked in [#373] | Covered for timeout and inner errors | Audit in [#372] | `tower::timeout::Timeout` |

`healthcheck` is not in the matrix because it does not implement a Tower
`Service`; it supplies a monitor/handle used by other layers. `core` supplies
shared infrastructure, and the facade crate only re-exports layer crates.

## Required assertions for new or changed services

Every new execution path must demonstrate, where applicable:

1. `poll_ready` is driven before every `call`, including calls on internal
   clones and retry attempts.
2. Readiness or admission is reserved across clones; multiple clones cannot
   all spend one permit.
3. pending readiness registers a waker and readiness errors are preserved.
4. dropping the returned future cancels owned work and releases resources.
5. losing hedge/fan-out work is cancelled before the winning future returns.
6. responses and errors are returned without lossy conversion.
7. edge configuration fails explicitly rather than panicking or busy-spinning.

[#361]: https://github.com/joshrotenberg/tower-resilience/issues/361
[#362]: https://github.com/joshrotenberg/tower-resilience/issues/362
[#363]: https://github.com/joshrotenberg/tower-resilience/issues/363
[#365]: https://github.com/joshrotenberg/tower-resilience/issues/365
[#366]: https://github.com/joshrotenberg/tower-resilience/issues/366
[#367]: https://github.com/joshrotenberg/tower-resilience/issues/367
[#368]: https://github.com/joshrotenberg/tower-resilience/issues/368
[#369]: https://github.com/joshrotenberg/tower-resilience/issues/369
[#371]: https://github.com/joshrotenberg/tower-resilience/issues/371
[#372]: https://github.com/joshrotenberg/tower-resilience/issues/372
[#373]: https://github.com/joshrotenberg/tower-resilience/issues/373
[#374]: https://github.com/joshrotenberg/tower-resilience/issues/374
[#375]: https://github.com/joshrotenberg/tower-resilience/issues/375
[#381]: https://github.com/joshrotenberg/tower-resilience/issues/381
[#382]: https://github.com/joshrotenberg/tower-resilience/issues/382
[#413]: https://github.com/tower-rs/tower/issues/413
[#855]: https://github.com/tower-rs/tower/pull/855
[#870]: https://github.com/tower-rs/tower/issues/870
[circuitbreaker-tower-comparison.md]: circuitbreaker-tower-comparison.md
