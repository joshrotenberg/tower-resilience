# Tower API surface audit

This is the durable record for #376: an audit of every Tower-overlapping
middleware crate against core Tower's own conventions across five axes --
`Service::Future` boxing, trait-bound tightness (`Send`/`Sync`/`'static`),
per-call allocations, Tokio coupling, and inner-service accessors. It exists
so a future change to any of these crates' public API surface can be checked
against a documented baseline instead of re-deriving the comparison from
scratch.

See the [Tower contract matrix](tower-contract-matrix.md) for the
regression-test-level (behavioral) view of the same crates, and
[circuitbreaker-tower-comparison.md](circuitbreaker-tower-comparison.md) for
the deep comparison against upstream's circuit-breaker proposal --
`circuitbreaker-tower-comparison.md` explicitly defers this API-surface
analysis here.

Tower source citations below are against `tower = "0.5.3"` (the version this
workspace depends on).

## Summary

| Crate | `Service::Future` | `S`/`Req` need `Send + 'static`? | `get_ref`/`get_mut`/`into_inner` | Tokio coupling in non-test code |
| --- | --- | --- | --- | --- |
| adaptive | Named (`AdaptiveFuture`) | `S::Future`/`S::Response`/`S::Error: Send + 'static`, but not `S` or `Req` themselves | `get_ref`/`get_mut` only -- see [below](#the-one-into_inner-exception) | `tokio::sync::Semaphore` + `tokio_util::sync::PollSemaphore` (admission) |
| bulkhead | `BoxFuture` | Yes | Added in this PR | `tokio::sync::Semaphore` + `tokio_util::sync::PollSemaphore` (admission) |
| cache | `BoxFuture` | Yes | Added in this PR | None -- had an unused non-dev `tokio` dependency, removed in this PR (see [below](#three-crates-had-an-unused-non-dev-tokio-dependency)) |
| chaos | `BoxFuture` | Yes | Added in this PR | `tokio::time::sleep` (latency injection) |
| circuitbreaker | `BoxFuture` (both `CircuitBreaker` and `CircuitBreakerWithFallback`) | Yes | Added in this PR (both types) | `tokio::sync::Mutex` (circuit state) + `tokio::time::Sleep` (backpressure mode) + `tokio::sync::Semaphore` (half-open admission) |
| coalesce | Named (`CoalesceFuture`) | Yes -- driven by `tokio::spawn`, not by boxing (see [below](#bounds-driven-by-spawning-not-boxing)) | Added in this PR | `tokio::spawn` (leader task detached from the initiating caller) |
| executor | Named (`ExecutorFuture`) | Yes | Pre-existing (only crate that already had the full triad before this PR) | `tokio::runtime::Handle` (pluggable `Executor` trait, not hardwired) |
| fallback | `BoxFuture` (both `Fallback` and `ServiceFallback`) | Yes | Added in this PR (`ServiceFallback` exposes the primary only -- see [below](#the-servicefallback-primary-only-accessor)) | none beyond `#[tokio::test]`; `ServiceFallback`'s shared backup uses `futures::lock::Mutex`, not Tokio's |
| hedge | `BoxFuture` | Yes | Added in this PR | `tokio::time::sleep` (hedge delay) |
| outlier | Named (`OutlierDetectionFuture`) -- de-boxed in #426 | No -- see [below](#de-boxing-prototype-outlier-426) | Added in the #376 PR | None -- had an unused non-dev `tokio` dependency, removed in the #376 PR (see [below](#three-crates-had-an-unused-non-dev-tokio-dependency)) |
| ratelimiter | `BoxFuture` | Yes | Added in this PR | `tokio::time::Sleep` (backpressure mode) |
| reconnect | Named (`ReconnectFuture`) | No -- see [below](#reconnect-and-router-no-single-inner-service) | Not applicable -- see [below](#reconnect-and-router-no-single-inner-service) | `tokio::time::Sleep` (reconnect backoff) |
| retry | `BoxFuture` | Yes | Added in this PR | `tokio::time::sleep` (backoff delay) |
| router | `S::Future` (pure pass-through, no wrapper at all) | No | Not applicable -- see [below](#reconnect-and-router-no-single-inner-service) | None -- had an unused non-dev `tokio` dependency, removed in this PR (see [below](#three-crates-had-an-unused-non-dev-tokio-dependency)) |
| timelimiter | `BoxFuture` | Yes | Added in this PR | `tokio::time::timeout` |

`healthcheck` is excluded (it supplies a monitor/handle used by other layers,
not a `tower::Service`), matching the contract matrix.

## Boxing and trait bounds

9 of the 15 `Service`-implementing crates (`bulkhead`, `cache`, `chaos`,
`circuitbreaker` x2, `fallback` x2, `hedge`, `ratelimiter`, `retry`,
`timelimiter`) set `type Future = BoxFuture<'static, Result<...>>`
(`futures::future::BoxFuture`, i.e. `Pin<Box<dyn Future<Output = ...> +
Send>>`). Boxing a trait object requires the boxed future to be `Send +
'static`, and Rust's type system propagates that requirement outward to
every type that future is built from -- which is why every one of those nine
crates' `Service` impl also requires `S: Send + 'static`, `S::Future: Send +
'static`, `Req: Send + 'static`, and usually `S::Response`/`S::Error: Send +
'static` too. For example (`crates/tower-resilience-ratelimiter/src/lib.rs`):

```rust,ignore
impl<S, Req> Service<Req> for RateLimiter<S>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    Req: Send + 'static,
{
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;
    ...
```

Core Tower's own equivalents make the opposite tradeoff: a hand-written,
`pin_project!`-based named future type that avoids both the allocation and
the bound propagation. `tower::limit::rate::RateLimit<S>`
(`tower-0.5.3/src/limit/rate/service.rs`) requires only `S: Service<Request>`
and reuses `S::Future` directly with **zero** additional bounds:

```rust,ignore
impl<S, Request> Service<Request> for RateLimit<S>
where
    S: Service<Request>,
{
    type Future = S::Future;
    ...
```

`tower::timeout::Timeout<S>` (`tower-0.5.3/src/timeout/mod.rs`) requires
only `S: Service<Request>, S::Error: Into<BoxError>` -- no `Send`, no
`'static` -- via a small named `ResponseFuture<F>` wrapping `S::Future` plus
a `Pin<Box<Sleep>>`. `tower::retry::Retry<P, S>`
(`tower-0.5.3/src/retry/mod.rs`) requires only `P: Policy<...> + Clone, S:
Service<Request> + Clone` via a `pin_project!`-generated `ResponseFuture<P,
S, Request>` state machine with three states (`Called`/`Waiting`/`Retrying`)
-- compare to this crate's `tower-resilience-retry::Retry`, whose bound set
above is a direct consequence of `BoxFuture`.

Five crates already avoid this: `adaptive` (`AdaptiveFuture`), `coalesce`
(`CoalesceFuture`), `executor` (`ExecutorFuture`), `outlier`
(`OutlierDetectionFuture`, de-boxed in #426 -- see
[below](#de-boxing-prototype-outlier-426)), and `reconnect`
(`ReconnectFuture`) each hand-write a named future type via `pin_project!`
or manual `Future` impls, matching Tower's own approach. `router` goes
further and doesn't wrap the future at all (`type Future = S::Future`),
identical in shape to `tower::limit::rate::RateLimit`.

### Is de-boxing the other nine crates an "avoidable restriction"?

Given the above, `Send + 'static` on `S`/`Req` in the nine remaining
`BoxFuture` crates is a direct, mechanical consequence of the boxing
choice, not an independently-chosen restriction -- so it is not something
that can be "removed" in isolation; removing it requires removing the
boxing itself. Doing that properly (a hand-written `pin_project!` state
machine per crate, each with its own poll-loop shape -- rejection vs
backpressure branching, sliding-window/circuit-state interaction,
cache-store locking, and so on) is a substantial, per-crate redesign with
real correctness risk for some of the nine (see the
[difficulty assessment](#remaining-crates-difficulty-and-recommendation)
below) -- which is why the original #376 PR scoped itself to auditing plus
the genuinely zero-risk fixes (accessors, below) and left this as follow-up
work. #426 explored that follow-up: it de-boxed `outlier` as a proof of the
pattern (the crate whose `call()` had the fewest branches and no internal
timer/lock state) and assessed the rest -- some are worth doing next, two
are probably not worth it on their own. See
[below](#de-boxing-prototype-outlier-426) for both.

### Bounds driven by spawning, not boxing

`coalesce` is the one exception where `Send + 'static` bounds exist despite
a named (non-boxed) future type: the leader request's work is driven via
`tokio::spawn` (`crates/tower-resilience-coalesce/src/lib.rs`) so that
waiters still receive a result even if the request that happened to become
"leader" is dropped by its own caller. `tokio::spawn` itself requires `F:
Future + Send + 'static`, so this bound is inherent to the coalescing
semantics (a detached task that outlives any single caller's future), not
to `CoalesceFuture`'s shape. This is intentional and not something to
"remove" -- coalescing without a detached leader task would mean a waiter's
result is lost whenever the leader's caller drops its future, defeating the
point of coalescing.

## Tokio coupling

Every crate in the table above except `router`, `cache`, `outlier`, and
`fallback`'s `Fallback`/value path (`ServiceFallback`'s shared backup uses
`futures::lock::Mutex`, not Tokio's) uses a concrete `tokio::sync::*` or
`tokio::time::*` type directly in its non-test source, not behind a
runtime-agnostic abstraction. This is a deliberate, crate-wide choice, not
an oversight: `tokio::sync::Semaphore` (bulkhead, adaptive, circuitbreaker's
half-open admission), `tokio::sync::Mutex` (circuitbreaker),
`tokio::time::Sleep`/`timeout`/`sleep` (ratelimiter and circuitbreaker
backpressure mode, timelimiter, hedge, reconnect backoff, chaos latency
injection, retry backoff delay), and `tokio::spawn` (coalesce) are all
load-bearing for the semantics those crates provide (fair FIFO admission,
cooperative cancellation-safe waiting, deterministic virtual-time-testable
delays). `executor` is the one crate that abstracts this behind a pluggable
`Executor` trait (with a `tokio::runtime::Handle` impl provided, not
hardwired), which is why it does not appear as "coupled" in the same sense
-- but even `executor` depends on `tokio` in its `Cargo.toml` for that
default impl and its own tests. No crate in this workspace is currently
runtime-agnostic (buildable against, say, `async-std` or `smol` without a
Tokio dependency at all), and un-coupling any of them would be a
substantially larger project than an API audit -- documented here as an
intentional tradeoff, not a gap to close.

### Three crates had an unused non-dev Tokio dependency

`cache`, `outlier`, and `router` each declared a non-dev `tokio` dependency
in `Cargo.toml`, but none of the three used any `tokio::` API in library
code -- every `tokio::` reference in all three crates was inside
`#[cfg(test)]` code, already covered by each crate's separate
`[dev-dependencies]` `tokio` entry:

- `cache`'s store lock is `std::sync::Mutex`
  (`crates/tower-resilience-cache/src/lib.rs`), not `tokio::sync::Mutex`.
- `outlier` and `router` have no locking or timing needs of their own at
  all (outlier delegates ejection timing to `crate::detector`'s
  `std::time::Instant` math; router is a pure `Vec<(S, u32)>` dispatch with
  no internal state beyond the selector).

This was an unnecessary compile-surface dependency for every downstream
user of these three crates -- a genuinely avoidable restriction with no
tradeoff, so it is fixed directly in this PR: the non-dev `tokio` line is
removed from each crate's `[dependencies]` (the existing
`[dev-dependencies]` entry already covers test needs in every case).
`cargo test -p tower-resilience-cache -p tower-resilience-outlier -p
tower-resilience-router` passes unchanged. Other crates' non-dev `tokio`
dependencies were checked against their real `tokio::` usage and found
legitimate (e.g. `chaos` and `retry` declare `tokio = { workspace = true }`
with no explicit feature list, relying on the workspace's default `["time",
"sync"]` features for their `tokio::time::sleep` calls). This audit did not
go further and try to minimize each crate's explicit *feature* list (e.g.
whether `bulkhead`'s explicit `"rt"` feature is actually load-bearing) --
that is a much narrower, more speculative optimization than "this
dependency is entirely unused," and out of scope here.

## Inner-service accessors

Core Tower's own middleware (`Timeout`, `RateLimit`, `Retry`, `Buffer`,
`Balance`, ...) uniformly exposes `get_ref(&self) -> &S`, `get_mut(&mut
self) -> &mut S`, and `into_inner(self) -> S` on the wrapping service. Before
this PR, only `tower-resilience-executor::ExecutorService` had this triad;
every other single-inner-service wrapper in this workspace was missing it
entirely -- there was no way to reach the wrapped service back out of a
`Bulkhead<S>`, `RateLimiter<S>`, `CircuitBreaker<S, C>`, etc. once
constructed, short of holding onto the original value separately before
wrapping it.

This PR adds the triad (or the reachable subset of it -- see the two
exceptions below) to every crate listed as "Added in this PR" in the
[summary table](#summary): `bulkhead`, `cache`, `chaos`, `circuitbreaker`
(`CircuitBreaker` and `CircuitBreakerWithFallback`), `coalesce`, `fallback`
(`Fallback` and `ServiceFallback`), `hedge`, `outlier`, `ratelimiter`,
`retry`, and `timelimiter`. This is purely additive -- new public methods,
no signature or behavior changes to anything existing -- so it carries no
tradeoff and needed no design discussion; each crate's test suite gained one
`accessors_expose_the_inner_service`-style test proving the methods compile
and return the expected value.

### The one `into_inner` exception

`AdaptiveService<S, A>` implements a custom `Drop` (to release a semaphore
permit reserved by `poll_ready` if the service is dropped before `call`).
Rust's partial-move rules (`E0509`) forbid moving a field out of `self` by
value when the containing type has a manual `Drop` impl, which blocks a
plain `into_inner(self) -> S { self.inner }`. Working around that requires
`unsafe` (`ManuallyDrop` plus manually re-implementing `Drop::drop`'s effect
for every other field so they still deallocate exactly once) for the sake of
one accessor method on one crate. That risk was judged not worth taking
here, so `AdaptiveService` is the one wrapper with `get_ref`/`get_mut` but
no `into_inner`.

### The `ServiceFallback` primary-only accessor

`tower-resilience-fallback::ServiceFallback<S, B, Req, Res, E>` holds a
primary service (`S`, owned directly) and a backup service (`B`, behind
`Arc<Mutex<B>>` and shared across every clone the layer produces, by
design -- see the type's own doc comment). The accessor triad added here
exposes the primary only (`get_ref`/`get_mut`/`into_inner` all return/consume
`S`); the shared backup does not fit the same by-value/by-reference shape
and is intentionally not exposed by a matching accessor set.

### `reconnect` and `router`: no single inner service

Two crates do not fit the single-`&S`-accessor shape at all, and are
intentionally left without a forced `get_ref`/`get_mut`/`into_inner`:

- **`reconnect`**: `ReconnectService<M, Target>` owns `Arc<Mutex<Shared<M,
  Target>>>`, where `M` is a `MakeService`-style factory shared across every
  clone (a reconnect that replaces a failed connection must be visible to
  every clone, not local to one). There is no synchronous `&M` to hand back
  without holding the lock, so a `get_ref` would need to either block
  synchronously on an async mutex or return a guard type, neither of which
  matches Tower's `&S`-returning convention. `state()`/`config()` already
  provide read access to the parts of `ReconnectService` that are meaningful
  to inspect from outside.
- **`router`**: `WeightedRouter<S>` wraps `Vec<(S, u32)>` -- a weighted
  collection of backends, not one inner service -- so there is no single `S`
  for `get_ref`/`get_mut`/`into_inner` to return. `backend_count()`,
  `weights()`, and `name()` already provide the inspection surface that
  makes sense for a collection.

Both of these are also the two best-in-class examples in the [boxing and
trait bounds](#boxing-and-trait-bounds) section above: `router`'s
`Service` impl requires nothing beyond `S: Service<Request>` and doesn't
wrap the future at all, and `reconnect`'s requires no `Send`/`'static` at
all either.

## Allocations beyond boxing

Per-call allocation beyond the `BoxFuture` heap allocation itself is
limited and mostly unavoidable given each crate's semantics: `hedge`
allocates a `FuturesUnordered` to drive parallel attempts (inherent to
racing N futures), `cache` and `coalesce` allocate map entries on a miss
(inherent to caching/coalescing), and `retry`/`circuitbreaker` clone the
request when a retry or half-open probe requires a fresh attempt (bounded
by `max_attempts`/`permitted_calls_in_half_open`, not unbounded). None of
these were found to be avoidable without changing the feature they
implement, so none are flagged as restrictions to remove.

## Follow-up

#426 tracks exploring de-boxing (`BoxFuture` to named, `pin_project!`-based
future types) crate by crate, as a follow-up to this audit -- mirroring how
#372's construction-time-validation audit scoped its first fix to
`circuitbreaker` (#417) and tracked the rest in #422.

### De-boxing prototype: `outlier` (#426)

`outlier` was chosen as the first de-boxing candidate: its `call()` has
exactly one branch point (error mode's pre-ejected immediate-error path
versus the normal wrapped-inner-future path) and no internal timer, lock, or
multi-attempt state, so it needed only a two-variant `pin_project!` enum
(`OutlierDetectionFuture<F, C>` in
`crates/tower-resilience-outlier/src/service.rs`) rather than a bespoke
poll-loop. The `Service` impl's bound list dropped from `S: Send + 'static,
S::Future: Send + 'static, S::Response: Send + 'static, S::Error: Send +
'static, C: ... + Send + 'static, Request: Send + 'static` down to just `S:
Service<Request> + Clone, C: FailureClassifier<S::Response, S::Error> +
Clone` -- the same bound set `tower::limit::rate::RateLimit` and this
workspace's other named-future crates carry.

**Allocation evidence.** `tests/outlier_deboxing_allocation_evidence.rs`
installs a counting `#[global_allocator]` in its own test binary (so it
cannot affect any other test's allocator) and measures one `call()` on the
de-boxed implementation against a byte-for-byte reconstruction of the
pre-#426 `BoxFuture`-based implementation (same clone/classify/record work,
differing only in `Box::pin` vs the named enum). Result, stable across five
repeated runs: **1 allocation per call de-boxed, versus 2 boxed** -- exactly
the `Box::pin` heap frame eliminated, with the remaining allocation being the
`instance_name: String` clone both implementations pay identically. This is
a small, real, per-call win, not a measurement artifact; the takeaway for
the crates below is that the boxing removal itself is worth roughly one
allocation per call, and the harder or riskier state-machine rewrites (which
some of the remaining nine are) should be weighed against that -- not
against some larger allocation-elimination number.

### Remaining crates: difficulty and recommendation

Assessed by reading each crate's `call()` body and existing state (not just
the summary table above). Ordered roughly easiest to hardest:

| Crate | Shape | Difficulty | Notes |
| --- | --- | --- | --- |
| `cache` | Two branches: cache hit (immediate `Ok(response)`, no inner future) vs. cache miss (wrap inner future, insert on completion). | Low | Structurally identical to `outlier`'s prototype -- a two-variant enum, no timers or locks held across `.await`. Good second candidate. |
| `bulkhead` | Backpressure/non-backpressure entry, one wrapped inner future with a semaphore permit released on completion, plus a "not ready" immediate-error branch. | Low-medium | Same overall shape as `outlier` (permit bookkeeping is synchronous, not something the future itself has to drive across polls). |
| `ratelimiter` | Backpressure mode wraps the inner future with permit/window bookkeeping; the harder admission decisions (token bucket, sliding window) already happen in `poll_ready`, not in the future. | Medium | Similar to `bulkhead` in shape; the existing `RateLimiterHandle` plumbing needs checking for anything that assumes a `'static` future. |
| `timelimiter` | Two independent modes: `cancel_running_future` (default) can reuse `tokio::time::timeout()`'s own named `Timeout<F>` type directly with no new code; `detached` mode spawns and races a `oneshot::Receiver` against a `Sleep`, the same shape `executor`'s `ExecutorFuture` already has plus one extra field. | Medium | Two variants, both with local precedent already in this workspace (`tokio::time::Timeout`, `ExecutorFuture`). Not hard, just two future types instead of one. |
| `chaos` | Sequential: optional injected latency (`tokio::time::sleep`) before calling inner, or immediate fault-injection error, or a direct pass-through. | Medium | A 2-3 variant enum with a `Sleep` field for the latency-injection case; no loops or shared locks. |
| `retry` | Multi-attempt loop: call, evaluate policy, optionally sleep for backoff, call again, up to `max_attempts`. | Medium | Core Tower's own `tower::retry::Retry` (`tower-0.5.3/src/retry/mod.rs`) already solves this exact shape with a 3-state `pin_project!` enum (`Called`/`Waiting`/`Retrying`) that this crate's version could mirror closely -- a proven template lowers the risk relative to its apparent complexity. |
| `fallback` (`Fallback` and `ServiceFallback`) | Two-phase sequential: await primary, and on a fallback-triggering result, await backup. `ServiceFallback`'s backup is behind `Arc<Mutex<B>>` (`futures::lock::Mutex`, itself poll-based), so the backup phase is its own nested lock-then-call sub-state-machine. | Medium-high | `Fallback`'s value/closure backup path is simpler (no second service future to project, no lock). `ServiceFallback` is harder because the async-mutex-guarded backup call needs its own state (acquire lock, then poll the locked service's future) layered inside the outer Primary/Backup enum. |
| `circuitbreaker` (`CircuitBreaker` and `CircuitBreakerWithFallback`) | Already the most complex `call()` in the workspace even in its current boxed form: a `tokio::sync::Mutex` for circuit state, a `tokio::sync::Semaphore` for half-open admission, and an `Option<Pin<Box<tokio::time::Sleep>>>` for backpressure mode, combined per-call. | High | The crate already hand-manages a boxed `Sleep` internally, which is a signal of real state-machine complexity, not just future-wrapping. `CircuitBreakerWithFallback` adds a second service future on top. This is the crate the original #376 audit explicitly called out as carrying the most correctness risk, and that assessment holds. |
| `hedge` | Races up to `max_hedged_attempts` inner-service calls concurrently via `FuturesUnordered` + `tokio::select!` against per-attempt delays, cancelling losers when a winner completes. | High | A hand-rolled equivalent needs to reimplement `FuturesUnordered`'s poll-many-and-remove-completed behavior manually, or restrict itself to a small `SmallVec`/array of `Option<Pin<&mut F>>` slots polled in a fixed-size loop (which pin-projects awkwardly for a runtime-determined `max_hedged_attempts`). Racing an unbounded set of heterogeneous futures without a heap-allocated collection is the least precedented shape in this workspace. |

**Recommendation:** `cache` and `bulkhead` are the next two candidates --
same low-risk shape as this PR's `outlier` prototype, each worth its own PR
per the per-crate-PR discipline above. `chaos`, `timelimiter`, and
`ratelimiter` are reasonable medium-difficulty follow-ups once the pattern
is well-worn. `retry` is medium risk but has a direct upstream template to
mirror (`tower::retry::Retry`), which is worth leaning on rather than
inventing a new shape.

`circuitbreaker` and `hedge` are the two crates most likely **not** worth
de-boxing on their own merits: both already carry meaningfully more
per-call state-machine complexity than a `Box::pin` removal saves (roughly
one allocation, per the measurement above), and `circuitbreaker` in
particular is under active, carefully-scoped work already (#372, #417,
#422) where introducing a hand-written poll loop is a correctness risk this
audit is not positioned to take on. If either is revisited, it should be
its own dedicated design effort weighing the one-allocation-per-call win
against the state-machine rewrite risk explicitly, not bundled into the
mechanical "de-box the next crate" pattern this table otherwise describes.
`fallback`'s `ServiceFallback` sits in between -- worth attempting after the
low-risk crates are done, but should budget for the nested lock-then-call
sub-state-machine, not just a plain enum.
