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
| outlier | `BoxFuture` | Yes | Added in this PR | None -- had an unused non-dev `tokio` dependency, removed in this PR (see [below](#three-crates-had-an-unused-non-dev-tokio-dependency)) |
| ratelimiter | `BoxFuture` | Yes | Added in this PR | `tokio::time::Sleep` (backpressure mode) |
| reconnect | Named (`ReconnectFuture`) | No -- see [below](#reconnect-and-router-no-single-inner-service) | Not applicable -- see [below](#reconnect-and-router-no-single-inner-service) | `tokio::time::Sleep` (reconnect backoff) |
| retry | `BoxFuture` | Yes | Added in this PR | `tokio::time::sleep` (backoff delay) |
| router | `S::Future` (pure pass-through, no wrapper at all) | No | Not applicable -- see [below](#reconnect-and-router-no-single-inner-service) | None -- had an unused non-dev `tokio` dependency, removed in this PR (see [below](#three-crates-had-an-unused-non-dev-tokio-dependency)) |
| timelimiter | `BoxFuture` | Yes | Added in this PR | `tokio::time::timeout` |

`healthcheck` is excluded (it supplies a monitor/handle used by other layers,
not a `tower::Service`), matching the contract matrix.

## Boxing and trait bounds

10 of the 15 `Service`-implementing crates (`bulkhead`, `cache`, `chaos`,
`circuitbreaker` x2, `fallback` x2, `hedge`, `outlier`, `ratelimiter`,
`retry`, `timelimiter`) set `type Future = BoxFuture<'static, Result<...>>`
(`futures::future::BoxFuture`, i.e. `Pin<Box<dyn Future<Output = ...> +
Send>>`). Boxing a trait object requires the boxed future to be `Send +
'static`, and Rust's type system propagates that requirement outward to
every type that future is built from -- which is why every one of those ten
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

Four crates already avoid this: `adaptive` (`AdaptiveFuture`), `coalesce`
(`CoalesceFuture`), `executor` (`ExecutorFuture`), and `reconnect`
(`ReconnectFuture`) each hand-write a named future type via `pin_project!`
or manual `Future` impls, matching Tower's own approach. `router` goes
further and doesn't wrap the future at all (`type Future = S::Future`),
identical in shape to `tower::limit::rate::RateLimit`.

### Is de-boxing the other ten crates an "avoidable restriction"?

Given the above, `Send + 'static` on `S`/`Req` in the ten `BoxFuture` crates
is a direct, mechanical consequence of the boxing choice, not an
independently-chosen restriction -- so it is not something that can be
"removed" in isolation; removing it requires removing the boxing itself.
Doing that properly (a hand-written `pin_project!` state machine per crate,
each with its own poll-loop shape -- rejection vs backpressure branching,
sliding-window/circuit-state interaction, cache-store locking, and so on) is
a substantial, per-crate redesign with real correctness risk (the four
crates that already do this run a meaningfully larger state machine than a
`Box::pin(async move { ... })` body). That is a bigger, riskier change than
belongs in this PR, which is scoped to auditing plus the genuinely
zero-risk fixes (accessors, below). #426 tracks de-boxing as future work,
crate by crate, starting with whichever crate's call-path is simplest.

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
