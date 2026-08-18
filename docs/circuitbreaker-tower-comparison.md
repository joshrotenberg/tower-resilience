# Circuit breaker vs. tower-rs/tower#855

This is the durable comparison checklist for `tower-resilience-circuitbreaker`
against the upstream circuit-breaker proposal at
[tower-rs/tower#855](https://github.com/tower-rs/tower/pull/855) (a
`tower::circuit_breaker::CircuitBreakerLayer` / `CircuitBreaker<S, P>` /
`CircuitPolicy` trait, authored by Matthew Busel, with design feedback from
Sean Monstar at
[#855#issuecomment-4039743246](https://github.com/tower-rs/tower/pull/855#issuecomment-4039743246)).
It exists so a reviewer can walk a future diff of that PR against this project
without redoing the analysis from scratch. This document reflects the
upstream diff as of commit `ddb88ba` (the `CircuitPolicy`-trait revision
pushed in response to the linked review comment) and the local API on `main`
after #381, #382 (PR #407), and #384 (PR #412).

Trait-bound, boxing, allocation, and runtime-coupling analysis (`Send`/`Sync`
requirements, `Arc<Mutex<_>>` vs. lock-free state, executor assumptions) is
tracked separately in #376 and is **not** duplicated here.

See the [Tower contract matrix](tower-contract-matrix.md) for the
regression-test-level view of the same coverage.

## API and state-machine comparison

Both projects converge on the same three-state model:

| | Local (`tower-resilience-circuitbreaker`) | Upstream `tower::circuit_breaker` (PR #855, commit `ddb88ba`) |
| --- | --- | --- |
| States | `CircuitState::{Closed, Open, HalfOpen}` (`circuit.rs`) | `CircuitStatus::{Closed, Open, HalfOpen}` (`service.rs`) -- same three states, different type name |
| Default trip condition | Failure **rate** over a count- or time-based sliding window (`FailureModel::SlidingWindow`, the default) | Consecutive-failure count only (`ConsecutiveFailures`, the only built-in `CircuitPolicy` impl) |
| Alternate trip condition | `FailureModel::ConsecutiveFailures { k }` -- k failures in a row, ignoring the sliding window entirely (`.consecutive_failures(k)`) | None built in; a custom `CircuitPolicy` impl would be required |
| Latency-based trip condition | `slow_call_duration_threshold` + `slow_call_rate_threshold`, evaluated independent of the failure-rate/consecutive path | None; would require a custom `CircuitPolicy` |
| Recovery timer | `wait_duration_in_open`; `Open -> HalfOpen` once elapsed (or never, in `manual_mode()`) | `timeout` constructor argument, same semantics (`ConsecutiveFailures::should_probe`) |
| Half-open admission size | `permitted_calls_in_half_open` (configurable, default 1), atomically reserved per cycle (#382) | Effectively unbounded per cycle in the current diff -- see [Admission semantics](#admission-semantics) |
| Half-open close condition | `success_count >= permitted_calls_in_half_open` within the current cycle; **any** failure reopens immediately | Rolling success-rate over up to 100 probe outcomes crosses `success_threshold`; **any** failure reopens immediately regardless of the rate (`future.rs`: "Any failure during a probe reopens immediately") |
| Config surface entry point | `CircuitBreakerConfigBuilder` (builder pattern, `.build()` / `.build_with_handle()`) plus `standard()` / `fast_fail()` / `tolerant()` presets | `CircuitBreakerLayer::new(failure_threshold, success_threshold, timeout)` (positional, `ConsecutiveFailures`-only) or `::with_policy(policy)` for a custom `CircuitPolicy` |
| Failure classification | `FailureClassifier<Res, Err>` trait, applied to the full `Result` (or just `Res` via `classify_response`) -- pluggable per call site, independent of the trip-condition model | None -- `on_success`/`on_failure` are called directly from whatever `Result` the inner service returned; there is no classifier seam distinguishing "this `Err` doesn't count as unhealthy" from "this `Err` does" |

## Admission semantics

- **Circuit state is checked before inner readiness.** Yes, since #381. The
  `Service` impl's `poll_ready` calls `poll_circuit_gate` first; for the
  common `Closed` case this is a lock-free atomic load with no mutex
  acquisition at all (`lib.rs`, `poll_circuit_gate`'s fast path). Contract
  coverage: `open_rejection_does_not_poll_pending_inner`,
  `cloned_open_breakers_reject_without_polling_inner`,
  `backpressure_waits_on_circuit_before_pending_inner` in
  `crates/tower-resilience-circuitbreaker/tests/contract.rs`.

- **Open-circuit rejection surfaces via `call`, not `poll_ready`, in the
  default (rejection) mode.** `poll_ready` grants readiness
  (`Poll::Ready(Ok(()))`) once the circuit gate decides to reject, recording
  the grant in a `reject_next_call` flag; the actual
  `CircuitBreakerError::OpenCircuit` is only returned from the following
  `call()` (`lib.rs`, `CircuitBreaker::poll_ready`/`call`). This matches how
  this crate's other rejecting layers (bulkhead, ratelimiter) behave, and
  avoids a `poll_ready` error tearing down stacks (e.g. `Buffer`) that treat a
  readiness error as fatal. In `.backpressure()` mode, rejection instead
  surfaces as `Poll::Pending` from `poll_ready` (never `Err`), covered by
  `backpressure_waits_on_circuit_before_pending_inner`. Upstream's diff takes
  the opposite default: `poll_ready` itself returns
  `Poll::Ready(Err(CircuitError::Open))` directly (`service.rs`), with no
  backpressure-style alternative.

- **Behavior across cloned services.** Circuit state is shared via
  `Arc<Mutex<Circuit>>` plus an `Arc<AtomicU8>` fast-path state cache;
  clones and every service produced by a `build_with_handle()` layer observe
  the same circuit. Covered by `cloned_open_breakers_reject_without_polling_inner`
  (#381) and by `test_handle_shared_across_cloned_services` /
  `test_handle_controls_boxed_and_moved_service` (#384, `handle.rs`) for the
  handle-decoupled case. Upstream's `CircuitBreaker<S, P>` is `#[derive(Clone)]`
  over an `Arc<Mutex<SharedState<P>>>`, giving the same shared-state property
  for direct clones, but has no service-independent handle type (see the
  [operator-control table](#operator-control-capability-comparison) below).

- **Concurrent half-open probes.** Reserved atomically: `Circuit::try_acquire`
  reserves a slot from a `tokio::sync::Semaphore` that is recreated fresh
  every time the circuit transitions into `HalfOpen` (so a permit released
  late by a probe from a stale cycle can never leak capacity into the current
  one), and dropping an admitted probe's future without completing it returns
  the reservation without recording a result (#382, PR #407). Covered by
  `half_open_admission_never_exceeds_permitted_calls_under_contention`,
  `dropped_half_open_probe_future_releases_its_slot_without_recording_a_result`,
  and `half_open_backpressure_waiter_wakes_promptly_when_a_slot_frees`.
  **Upstream's current diff does not gate concurrent half-open admission at
  all.** Once `SharedState.status` is set to `HalfOpen`, `poll_ready` falls
  through unconditionally to `self.inner.poll_ready(cx)` with no per-cycle
  admission check (`service.rs`) -- every concurrent caller is admitted
  during `HalfOpen`, limited only by whatever concurrency the caller
  otherwise has. This contradicts the module doc's own framing ("one probe
  request is allowed through"); the code does not currently enforce it.

## Does upstream `CircuitPolicy` support externally triggered circuits?

**Partial.** The built-in surface supports one direction only:
`CircuitBreaker::reset()` (service.rs) manually forces the circuit closed
(via the policy's `on_half_open()` window-clear hook), reachable only by
holding or cloning the concrete `CircuitBreaker<S, P>` value -- there is no
decoupled handle type, and no `force_open()` at all anywhere in the diff. A
custom `CircuitPolicy` implementation *could* be written to consult an
externally-set flag (e.g. an `Arc<AtomicBool>` captured in the policy struct)
from `should_probe()` / `on_failure()`, achieving a hand-rolled external
on/off switch -- which is exactly the "simple shared on-off switch" scope
Sean Monstar's review comment described as in-scope for a circuit breaker
concept. But that construction is left entirely to the policy author; it is
not a first-class API the trait or service provides. This project's
`.manual_mode()` + `CircuitBreakerHandle::force_open()`/`force_closed()`/`reset()`
(#384, PR #412) is the equivalent finished feature.

## Separation of concerns

Three conceptually distinct questions, deliberately kept in different places:

1. **Failure classification** -- "was this completed call's outcome a
   failure?" Lives in `classifier.rs` (re-exporting
   `tower_resilience_core::classifier::{FailureClassifier, DefaultClassifier, FnClassifier}`).
   Invoked once per completed call from `CircuitBreaker::call()` in `lib.rs`
   (`config.failure_classifier.classify(&result)`), independent of the
   trip-condition model below.
2. **State-transition policy** -- "given recorded outcomes (and elapsed
   time), should the circuit move `Closed -> Open`, `Open -> HalfOpen`, or
   `HalfOpen -> {Closed, Open}`?" Lives in `circuit.rs`
   (`Circuit::record_success` / `record_failure` / `evaluate_window` /
   `transition_to`), driven by `FailureModel`, the sliding-window
   configuration, and the slow-call thresholds.
3. **Request-admission policy** -- "given the *current* state, is this
   specific request allowed through right now?" Lives in `circuit.rs`
   (`Circuit::try_acquire` / `check_permitted`, including the half-open
   `Semaphore` reservation) and in the `Service` impl's `poll_circuit_gate` /
   `poll_ready` / `call` in `lib.rs`, which is also where rejection-mode vs.
   backpressure-mode is decided.

Upstream's `CircuitPolicy` trait bundles (1) and (2) together (a policy's
`on_success`/`on_failure` both classify *and* decide whether to trip, since
there is no separate classifier seam) and leaves (3) entirely inside
`CircuitBreaker::poll_ready`/`ResponseFuture::poll`, with no separately
testable admission type analogous to `Circuit::try_acquire`.

## Operator-control capability comparison

| Capability | Local | Upstream (PR #855, commit `ddb88ba`) |
| --- | --- | --- |
| Force-open | Yes -- `CircuitBreaker::force_open()` / `CircuitBreakerWithFallback::force_open()` / `CircuitBreakerHandle::force_open()` (`handle.rs`), deterministic: `Release`-ordered store before the call returns, so a subsequent `Acquire`-ordered read is guaranteed to observe it | No -- no force-open method anywhere in the diff |
| Force-closed | Yes -- `force_closed()` on all three surfaces above | No -- only `reset()`, which is a plain close, not a distinct "force closed" |
| Reset | Yes -- `reset()` on all three surfaces; also clears sliding-window counters | Partial -- `CircuitBreaker::reset()` exists (calls the policy's `on_half_open()` hook, then sets `Closed`) |
| Shared handle decoupled from the service instance | Yes -- `CircuitBreakerHandle` (`build_with_handle()`) holds its own `Arc` to the shared circuit and keeps controlling every service the layer produced after they are moved, boxed, or dropped (#384, PR #412; `test_handle_controls_boxed_and_moved_service`) | No -- the only control surface is `CircuitBreaker<S, P>` itself; cloning it still carries a clone of the inner service `S` |
| External health signals | Yes -- `HealthTriggerable` impl for `CircuitBreaker` / `CircuitBreakerWithFallback` / `CircuitBreakerHandle` under the `health-integration` feature (`health_integration.rs`), plus the deterministic, awaitable `force_open`/`force_closed` alternative | No built-in integration; would require a custom `CircuitPolicy` reading an external flag |
| External-only / manual operation | Yes -- `.manual_mode()` (#384, PR #412): counters/events still recorded for observability, but state changes only via explicit `force_open`/`force_closed`/`reset` | No equivalent mode; would require a custom `CircuitPolicy` whose `on_success`/`on_failure`/`should_probe` always return `false` |

## RMQTT and GovCraft/Acton integration review

**RMQTT** ([rmqtt/rmqtt](https://github.com/rmqtt/rmqtt),
`rmqtt/src/grpc.rs` + `rmqtt/src/context.rs`) and
**rmqtt-storage** ([rmqtt/rmqtt-storage](https://github.com/rmqtt/rmqtt-storage),
`src/circuit_breaker.rs`) both wrap the crate around outbound gRPC/storage
calls the same way:

- Build a `CircuitBreakerLayer` from a small internal config struct via
  `failure_rate_threshold`, `sliding_window_size`/`sliding_window_duration`,
  `wait_duration_in_open`, `slow_call_duration_threshold`, and
  `slow_call_rate_threshold` -- the sliding-window rate and slow-call axes,
  no `consecutive_failures`.
- Hold the concrete `CircuitBreaker<S, DefaultClassifier>` value directly
  (`TowerCB<GrpcSendService, DefaultClassifier>` in RMQTT) and read state via
  its own sync/async inspection methods (`state_sync()`, `is_open()`,
  `metrics()`) rather than `build_with_handle()` / `CircuitBreakerHandle` --
  workable because they never type-erase the service away from the code that
  needs to inspect it.
- Neither uses `force_open`/`force_closed`/`reset`/`manual_mode` (all
  predate #384 by less than a day at the time of this review, so this is not
  surprising) or a custom `failure_classifier`.

**GovCraft/Acton** ([Govcraft/acton-service](https://github.com/Govcraft/acton-service),
`acton-service/src/middleware/resilience.rs`) wraps the crate for **inbound**
axum HTTP middleware, which is a materially different integration shape:

- Two separate layer builders: `circuit_breaker_layer()` for outbound client
  stacks (default `Err`-only classification) and
  `http_circuit_breaker_layer()` for the inbound router, which supplies a
  custom `failure_classifier` (via `FnClassifier`) that treats any 5xx
  `Response` as a failure -- necessary because an axum route handler is
  `Infallible` (a 500 arrives as `Ok(Response)`, not `Err`), so the default
  classifier would never trip on an inbound stack.
- Always calls `build_with_handle()` and discards the handle (`.0`), because
  axum's `Router::layer` / `HandleErrorLayer` composition re-invokes
  `Layer::layer` per request in their setup, and a plain `build()` would mint
  fresh (i.e., never-tripping) circuit state on every request. This is a
  general Tower-layer-with-axum caveat (their own `bulkhead_layer()` has the
  identical comment), not specific to the circuit breaker.
- Also does not use `force_open`/`force_closed`/`manual_mode` or a shared
  `CircuitBreakerHandle`.

**Finding: no genuine API friction identified.** Both integrations configure
the rate/slow-call axes directly, and RMQTT's direct-service-inspection
pattern and Acton's custom-classifier-for-infallible-inbound-routes pattern
are both first-class, documented ways to use this crate today (the latter is
exactly what `classify_response`/`failure_classifier` exist for). No
workaround, wrapper-around-a-gap, or unsupported-need was found in either
codebase. No issue was filed.

## Ideas worth adopting from upstream

- Upstream's `on_half_open()` policy hook is an explicit, separately named
  callback for "clear stale pre-outage history when a recovery probe
  starts." Locally this happens implicitly inside `Circuit::transition_to`
  (unconditional counter/window reset on every transition). The behavior is
  equivalent, but a named hook is a slightly more readable place to hang
  future customization (e.g. a policy that wants to *keep* some
  pre-outage history) if `FailureModel` ever grows a fully pluggable variant.
- Upstream's `CircuitPolicy` trait (four small methods:
  `on_success`/`on_failure`/`should_probe`/`on_half_open`) is a reasonable
  reference shape *if* this project ever wants a fully custom, user-supplied
  trip policy beyond the `FailureModel` enum's rate/consecutive/slow-call
  axes. Nothing here currently requires it -- `FailureModel` plus the
  classifier seam already cover the practical cases -- but it's a clean
  small-trait design worth remembering if that need arises.

## Intentional capability differences

- **Slow-call detection** (`slow_call_duration_threshold` +
  `slow_call_rate_threshold`) as a first-class, independently-evaluated trip
  axis. Upstream's built-in `ConsecutiveFailures` policy has no latency
  concept at all.
- **Sliding-window failure *rate*** (count- or time-based) as the default
  trip model, vs. upstream's built-in policy being consecutive-count-only
  (a rate policy would have to be hand-written against `CircuitPolicy`).
- **Atomic per-cycle half-open admission** via a `Semaphore` recreated on
  every `Open -> HalfOpen` transition (#382), enforced at admission time --
  vs. upstream's current diff not gating concurrent `HalfOpen` admission at
  all (see [Admission semantics](#admission-semantics)).
- **`CircuitBreakerHandle`**: a fully decoupled, `Clone + Send + Sync`
  external control/observation surface independent of the wrapped service
  instance -- vs. upstream's only surface being the `Service` value itself.
- **`.manual_mode()`**: a first-class, declarative external-only mode --
  upstream would require hand-writing a `CircuitPolicy` that never trips or
  recovers on its own.
- **Fallback composition** (`with_fallback`): an open circuit can return an
  alternative response instead of an error. Upstream has no fallback
  concept; callers would layer their own.
- **Observability**: event listeners (`on_state_transition`,
  `on_call_permitted`, `on_call_rejected`, `on_success`, `on_failure`,
  `on_slow_call`) plus `metrics`/`tracing` feature integration. None of this
  exists in the upstream diff.
- **Preset configurations** (`standard()`, `fast_fail()`, `tolerant()`) for
  zero-tuning startup. Upstream's constructor is three positional arguments
  with no presets.
- **Pluggable failure classification** independent of the trip-condition
  model (see [Separation of concerns](#separation-of-concerns)). Upstream
  has no classifier seam.

## Composition with retry budgets

This section documents how circuit breaker and retry-budget (#370, PR #411)
composition works when both are layered on the same client stack, per this
issue's requirement to coordinate with #370.

### Layer order changes which attempts the breaker counts

Tower layers wrap from the outside in; the order below is
`ServiceBuilder::new().layer(A).layer(B).service(inner)`, where `A` is
outermost (sees the request first) and `B` is innermost (closest to
`inner`).

**Retry wrapping breaker** (breaker innermost, closest to the transport):

```rust,ignore
let service = ServiceBuilder::new()
    .layer(retry_layer)      // outermost
    .layer(circuit_breaker_layer)
    .service(transport);
```

Every retry attempt -- the initial attempt and every subsequent retry --
calls `poll_ready`/`call` on the breaker directly. Each attempt therefore
increments the breaker's sliding window independently: a request that fails
twice and succeeds on its third attempt records **two failures and one
success** in the breaker's window, exactly as if three independent calls had
been made. This is the natural, common ordering: the breaker protects the
transport from retry amplification, and it sees (and can trip on) the actual
retry volume hitting the backend.

**Breaker wrapping retry** (breaker outermost, sees only the retry's final
outcome):

```rust,ignore
let service = ServiceBuilder::new()
    .layer(circuit_breaker_layer) // outermost
    .layer(retry_layer)
    .service(transport);
```

The breaker only ever sees one call per external request: `Retry`'s own
`Service::call` future runs the entire retry loop internally and resolves
once, so the breaker's classifier and sliding window observe exactly one
outcome per external request -- a request that failed twice internally but
eventually succeeded records **one success**, not two failures and a
success. The breaker is effectively blind to the actual attempt volume
against the transport and can only react to the retry policy's fully
adjudicated final answer. This ordering also does not gate individual retry
attempts on transport health at all -- if the breaker is closed based on
the smoothed final-outcome view, every internal retry attempt still reaches
the transport even while it is actively failing.

### Interaction with the retry budget's token accounting

Reading `crates/tower-resilience-retry/src/lib.rs` and
`crates/tower-resilience-retry/src/budget.rs`: the retry budget is
consulted only inside `Retry::call`'s loop, at the point a *subsequent*
attempt is being considered (never for the first attempt of a request), via
`budget.try_withdraw()`, and is only replenished via `budget.deposit()` on
eventual success. `try_withdraw`/`deposit` do not distinguish *why* an
attempt failed -- there is no special case for a `CircuitBreakerError`.

This matters differently depending on layer order:

- **Retry wrapping breaker** (breaker innermost): a breaker-rejected attempt
  (`CircuitBreakerError::OpenCircuit`) is indistinguishable from any other
  retryable error to `Retry`'s default `RetryPolicy` (which retries all
  errors by default -- `should_retry` returns `true` unless a
  `.retry_on(predicate)` builder call says otherwise). By default, an open
  circuit therefore **does consume a retry-budget token per rejected
  attempt**, even though the rejection never reached the network and cost
  nothing but a mutex lock and an atomic load. Left unconfigured, this can
  drain the budget on rejections alone during an outage, starving budget for
  requests that would otherwise succeed once the circuit half-opens.
- **Breaker wrapping retry** (breaker outermost): the breaker's rejection
  happens before `Retry` is ever reached (it fails at the breaker's own
  `poll_ready`/`call`, never entering the retry loop), so no budget
  token is withdrawn or deposited for a breaker rejection at all -- the
  retry budget only ever sees fully-adjudicated final outcomes from
  requests the breaker actually admitted.

**Recommendation for the retry-wraps-breaker ordering:** configure
`RetryLayer::builder().retry_on(predicate)` to return `false` for
`CircuitBreakerError::OpenCircuit` (check via
`CircuitBreakerError::is_circuit_open()`), so open-circuit rejections fail
the current attempt immediately without consuming a budget token or
scheduling a backoff sleep for a call that never left the process. The
current code does not do this automatically; it is a per-integration
configuration decision, not a bug -- `Retry` has no way to know that a given
`E` came from a wrapped circuit breaker without the caller telling it via
the predicate.

### Recommended default ordering

**Retry wrapping breaker (breaker innermost) is the recommended default**,
with the retry predicate excluding circuit-open rejections as described
above. This ordering lets the breaker observe and react to the actual retry
volume against the transport (the thing it exists to protect), keeps the
breaker's sliding window meaningful (it reflects real attempts, not
smoothed per-request outcomes), and -- once the predicate exclusion is
configured -- avoids wasting retry budget and backoff delay on calls that
were rejected locally rather than failing against the backend. The
alternative ordering (breaker outermost) is appropriate only when the
retry's *entire* multi-attempt operation should be treated as one unit of
health for the breaker's purposes (for example, when downstream jitter makes
individual-attempt failure rate a noisy signal and only the request-level
success/failure matters) -- but that reduced attempt-level visibility, and
the fact that it does not gate individual internal retry attempts on
transport health, should be a deliberate choice, not the default.
