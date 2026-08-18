# Downstream users and the adoption feedback loop

This is the durable tracking doc for #379 ("Build a feedback loop with
verified downstream users"). It exists so a maintainer can see, at a glance,
who actually depends on this project at runtime, how confident that claim is,
and what to ask each user once outreach is approved.

**No outreach has been sent.** Everything under [Outreach draft](#outreach-draft-not-sent)
is unsent text pending explicit maintainer approval, per this issue's own
scope note ("seek validation ... after outreach wording is approved"). This
audit itself is read-only: it was produced entirely from public crates.io
metadata and public GitHub repository contents (Cargo manifests, source
files, GitHub code search), with no contact made to any project outside this
repository.

See also [`docs/circuitbreaker-tower-comparison.md`](circuitbreaker-tower-comparison.md#rmqtt-and-govcraftacton-integration-review),
which already contains a circuit-breaker-specific RMQTT/Acton review (PR
#418). This document is broader: it covers every workspace crate, adds the
bulkhead review, and adds the classification/outreach/template scaffolding
that issue asks for.

## Evidence classes

Not all "depends on tower-resilience" signals mean the same thing. Each
entry below is tagged with exactly one of these, chosen from strongest to
weakest evidence and cited with the file(s) that back the claim:

| Class | What it means | How it was checked |
| --- | --- | --- |
| **Runtime, confirmed** | A direct (non-dev) Cargo dependency, and a source file was found that actually constructs and applies the layer/service (`.layer(...)`, `Foo::builder()...build()`/`build_with_handle()`) | `gh api search/code` for the crate's Rust identifiers (e.g. `tower_resilience_bulkhead`) scoped to the repo, then the matching file read directly |
| **Runtime, optional/feature-gated** | Same as above, but the downstream project marks the dependency `optional = true` and gates it behind its own Cargo feature -- the integration ships but isn't compiled into every build | The downstream `Cargo.toml`'s `[dependencies]` and `[features]` tables |
| **Dev-dependency only** | Declared under `[dev-dependencies]` -- used in the downstream project's own tests/benches, never in its shipped runtime path | crates.io reverse-dependency `kind` field (`dev` vs `normal`) |
| **Declared, unconfirmed** | Appears in the downstream `Cargo.toml`/`Cargo.lock`, but no source-level construction was found via code search | Same code search as "runtime, confirmed," returning zero source-file hits |
| **Mention-only** | The crate name appears in prose (README, architecture docs, blog post) with no corresponding Cargo dependency at all | `gh api search/code` hit landed in a `.md` file, not `Cargo.toml` or `.rs` |
| **First-party / same-maintainer** | The downstream repository is authored or primarily maintained by this project's own maintainer (Josh Rotenberg) | GitHub contributor list for the downstream repo |

crates.io's reverse-dependency data (`GET /api/v1/crates/<name>/reverse_dependencies`)
was pulled for all 18 workspace crates on 2026-08-18 to build the candidate
list below; code search and direct file reads then confirmed or downgraded
each candidate into one of the classes above.

## Verified independent downstream users

These are the two runtime-confirmed, third-party (not maintainer-affiliated)
users. Both are pre-existing, independently discovered adopters -- neither
was solicited.

### RMQTT (`rmqtt/rmqtt`, `rmqtt/rmqtt-storage`)

- **Maintainer:** `bittcrafter` (806 of 806+ contributions to `rmqtt/rmqtt`
  at time of review) -- no affiliation with this project's maintainer.
- **Patterns used:** circuit breaker only. crates.io shows no reverse
  dependency from either `rmqtt` repo on `tower-resilience-bulkhead`,
  `-retry`, `-ratelimiter`, `-timelimiter`, `-reconnect`, `-hedge`, or any
  other workspace crate.
- **`rmqtt/rmqtt`** (`rmqtt/Cargo.toml:77`): `tower-resilience-circuitbreaker.workspace = true`,
  **not** marked `optional` -- **runtime, confirmed, mandatory** (always
  compiled into the core `rmqtt` binary). Used in `rmqtt/src/grpc.rs` and
  `rmqtt/src/context.rs` per the existing review in
  [`circuitbreaker-tower-comparison.md`](circuitbreaker-tower-comparison.md#rmqtt-and-govcraftacton-integration-review).
- **`rmqtt/rmqtt-storage`** (`Cargo.toml:49`): `tower-resilience-circuitbreaker = { version = "0.10", optional = true }`,
  gated behind its own `circuit-breaker` feature (`Cargo.toml:29`) --
  **runtime, confirmed, optional/feature-gated**. Used in
  `rmqtt-storage/src/circuit_breaker.rs`.
- **Version currently pinned (main branch, both repos):** `"0.10"` /
  `"0.10.0"` -- see [0.11+ changes needing downstream validation](#011-changes-needing-downstream-validation)
  below.

### GovCraft/Acton (`Govcraft/acton-service`)

- **Maintainer:** `rrrodzilla` (388 of ~400 contributions; the remainder is
  `claude[bot]`, i.e. AI-assisted commits under the same human maintainer) --
  no affiliation with this project's maintainer.
- **Patterns used:** circuit breaker and bulkhead, both **runtime,
  confirmed, optional/feature-gated**:
  - `acton-service/Cargo.toml:98-99`: `tower-resilience-circuitbreaker = { workspace = true, optional = true }`
    and `tower-resilience-bulkhead = { workspace = true, optional = true }`,
    each individually feature-gated (`Cargo.toml:201-202`) and also bundled
    into a composite `resilience` feature (`Cargo.toml:178`).
  - Both are constructed and applied in
    `acton-service/src/middleware/resilience.rs`. Circuit breaker usage was
    already reviewed in
    [`circuitbreaker-tower-comparison.md`](circuitbreaker-tower-comparison.md#rmqtt-and-govcraftacton-integration-review);
    the bulkhead half of that file is new to this review -- see
    [Bulkhead friction review](#bulkhead-friction-review-govcraftacton) below.
  - Root `Cargo.toml` currently pins `tower-resilience-circuitbreaker = "0.10.0"`
    and `tower-resilience-bulkhead = "0.10.0"` (lines 89-90) -- same
    not-yet-on-0.11+ situation as RMQTT.
- **Same-org repos with a declared but unconfirmed dependency:**
  `GovCraft/acton-dx` and `GovCraft/acton-htmx` both declare
  `tower-resilience = "0.3"` in their root `Cargo.toml` (a version four
  major bumps behind current), but `gh api search/code` for
  `tower_resilience` inside either repo returns zero source-file hits.
  These read as scaffold/template repos in the same family as
  `acton-service` (same org, same Cargo.toml shape) that inherited the
  dependency declaration without using it -- **declared, unconfirmed**, not
  counted as additional independent adoption.

## Mention-only

- **`CoderByBlood/whisper-cms`** -- `tower-resilience-circuitbreaker` is
  named twice in `docs/arc42/08_crosscutting_concepts.md` (an architecture
  document) as a candidate resilience library alongside `failsafe`. No
  `Cargo.toml` or `.rs` reference exists anywhere in the repo. **Mention-only** --
  not a dependency, confirmed or otherwise. Included here for completeness,
  not as an adoption signal.
- A GitHub repository-search for "tower-resilience" in READMEs/descriptions
  returns mostly false positives (unrelated "tower" + "resilience" word
  matches in AWS study guides, blog aggregators, etc.); none were
  substantive enough to include.

## First-party / same-maintainer usage

These all resolve to repositories where Josh Rotenberg (this project's
maintainer) is the sole or overwhelming majority contributor. They're real
dogfooding and worth keeping in view, but they are **not independent
adoption evidence** and are explicitly excluded from the outreach list --
there's no one else to ask.

| Project | Repo | Class | Notes |
| --- | --- | --- | --- |
| tower-mcp | `joshrotenberg/tower-mcp` | Runtime, optional | Depends on the `tower-resilience` facade crate, optional |
| cratesio-mcp | `joshrotenberg/cratesio-mcp` | Runtime, optional | Same facade crate, optional |
| mcp-proxy | `joshrotenberg/mcp-proxy` | Runtime, mandatory + dev | Non-optional dependency on the facade crate; also a dev-dependency of `tower-resilience-chaos` |
| redisctl | `redis/redisctl` (862/873 commits by `joshrotenberg`) | Declared, unconfirmed | `crates/redisctl/Cargo.toml:62`: `tower-resilience = { version = "0.1", features = ["circuitbreaker", "retry", "ratelimiter"] }`, non-optional. Its own changelog records "add tower-resilience integration framework" (PR #459), but `gh api search/code` for `tower_resilience`, `CircuitBreaker`, `RetryLayer`, `RateLimiter` inside the repo returns zero `.rs` hits -- framework scaffolding, not yet wired to a command path as of this review. |
| redis-cloud | `redis-developer/redis-cloud-rs` (87/87 commits by `joshrotenberg`) | Dev-dependency only | `tower-resilience` appears only under `[dev-dependencies]`, req `^0.3.8` -- test-only, never shipped |

## Friction review

### Circuit breaker (RMQTT + Acton)

Already reviewed in full in
[`circuitbreaker-tower-comparison.md`](circuitbreaker-tower-comparison.md#rmqtt-and-govcraftacton-integration-review)
(PR #418). Restating only the conclusion here to avoid duplicating that
analysis: **no genuine API friction identified.** Both projects configure
the rate/slow-call axes directly; RMQTT's direct-service-inspection pattern
(`state_sync()`, `is_open()`, `metrics()` on the concrete
`CircuitBreaker<S, C>` value) and Acton's custom-classifier-for-infallible-inbound-routes
pattern (`FnClassifier` treating 5xx as failure) are both first-class,
already-documented ways to use the crate. No issue was filed.

### Bulkhead friction review (GovCraft/Acton)

New to this review -- RMQTT does not depend on `tower-resilience-bulkhead`
at all (confirmed via crates.io reverse dependencies, both `rmqtt` repos),
so there is nothing to review there.

Acton's usage (`acton-service/src/middleware/resilience.rs`,
`ResilienceConfig::bulkhead_layer()` and `apply_resilience()`):

- Uses the full builder surface as documented: `.name()`,
  `.max_concurrent_calls()`, `.max_wait_duration()`, `.on_call_permitted()`,
  `.on_call_rejected()`.
- Correctly calls `.build_with_handle()` rather than `.build()`, with the
  same rationale as their circuit breaker usage: axum re-invokes
  `Layer::layer` per request, and a plain `build()` would mint a fresh,
  always-empty semaphore on every request, silently disabling the
  concurrency cap. Their own code comment states this explicitly and cites
  `apply_resilience` -- this is the same axum-composition caveat covered in
  the circuit breaker review, applied consistently.
- Uses the default (queued/rejecting) mode via `max_wait_duration`, not
  `.backpressure()`. This is a deliberate, supported choice -- inbound HTTP
  wants a bounded wait then an explicit 503, not an unbounded
  `Poll::Pending` -- and matches how they use the circuit breaker's default
  rejection mode rather than its backpressure mode.
- Composition order is deliberate and documented: bulkhead applied
  innermost so a concurrency rejection becomes a 503 that the circuit
  breaker (applied outermost) then counts as a failure via its
  5xx-classifying `FnClassifier`. This lets sustained overload trip the
  breaker, not just shed load at the bulkhead.
- Does not use `BulkheadHandle` (the crate's external-inspection/`build_with_handle()`-returned
  handle type) for anything beyond the two callbacks above -- no gap, just
  an unused capability.

**Finding: no genuine API friction identified.** The full public builder
surface (concurrent-call cap, wait duration, both callbacks,
`build_with_handle()`) covers Acton's inbound-HTTP concurrency-limiting use
case without a workaround. The unreleased internal rewrite of backpressure
admission (permits polled and reserved directly instead of spawning one
Tokio task per waiter -- currently on `main`, not yet published) is a
behavior-preserving internal change from any caller's perspective and does
not affect this conclusion. No issue was filed.

### Other patterns searched

crates.io reverse dependencies were checked for all 18 workspace crates
(`tower-resilience-{retry,ratelimiter,timelimiter,reconnect,fallback,hedge,router,outlier,healthcheck,adaptive,cache,chaos,coalesce,executor,core}`)
against both RMQTT repos and all four GovCraft repos found. Neither
integration depends on anything beyond circuit breaker (both) and bulkhead
(Acton only) -- there is no retry, rate limiter, hedge, or other pattern
usage to review in either codebase today.

## 0.11+ changes needing downstream validation

`tower-resilience-circuitbreaker` 0.11.0 shipped one behavioral change:
**"gate inner readiness on circuit state"** (#386) -- `poll_ready` now
checks the circuit's gate *before* polling the inner service's readiness,
where previously the inner service's readiness was polled unconditionally.
This is documented in the
["Circuit state is checked before inner readiness"](circuitbreaker-tower-comparison.md#admission-semantics)
bullet of the comparison doc.

**Neither reviewed downstream project has picked this up yet.** As of this
audit, both `rmqtt/Cargo.toml` and `acton-service`'s root `Cargo.toml` pin
`tower-resilience-circuitbreaker = "0.10"` / `"0.10.0"` -- a caret
requirement that would resolve to 0.10.x only, not 0.11 or the current
0.12.0. Their integration code (RMQTT's `state_sync()`/`is_open()`
inspection; Acton's dual outbound/inbound layer builders) has not been
exercised against the new admission ordering.

This is the concrete thing worth asking each maintainer to validate once
outreach is approved: bump to `^0.12`, run their existing test suite, and
confirm no behavior difference around readiness/admission ordering. It is
also worth mentioning (not yet released, so framed as "coming, not
available yet") the unreleased `CircuitBreakerHandle::force_open()`/`force_closed()`/`reset()`
and `.manual_mode()` additions currently on `main` -- neither project
currently uses external circuit control, but RMQTT's direct-inspection
pattern in particular is exactly the shape that handle exists for.

## Usage-report template

See [`docs/downstream-usage-report-template.md`](downstream-usage-report-template.md).
A short, copy-pasteable template for a downstream maintainer to fill out
describing their integration, any friction, and (optionally) consent to
being named publicly. Intended to be linked from the outreach messages
below, or reused standalone for any future downstream conversation.

## Outreach draft (NOT sent)

**Status: draft only, pending maintainer approval.** Nothing below has been
posted, emailed, or otherwise sent to anyone outside this repository. Per
this issue's scope, outreach happens only after the wording is approved.

Suggested channel for both: a new issue on the downstream project's own
issue tracker (public paper trail, no email address needed, matches how
both projects already track their own feature work). Alternative: a comment
on whichever existing issue/PR originally added the tower-resilience
integration, if the maintainer prefers a lower-visibility thread.

### Draft message: RMQTT (`rmqtt/rmqtt` and `rmqtt/rmqtt-storage`, maintainer `bittcrafter`)

> Subject: tower-resilience circuit breaker integration -- quick check-in
>
> Hi -- I maintain [tower-resilience](https://github.com/joshrotenberg/tower-resilience),
> which `rmqtt` and `rmqtt-storage` both use for circuit breaking
> (`rmqtt/src/grpc.rs` and `rmqtt/src/context.rs`, and
> `rmqtt-storage/src/circuit_breaker.rs`). I wanted to reach out directly
> rather than assume anything from the outside.
>
> A few things:
>
> 1. Both repos currently pin `tower-resilience-circuitbreaker = "0.10"`.
>    0.11 changed `poll_ready` to check circuit state before inner-service
>    readiness (previously it polled the inner service unconditionally).
>    If you get a chance to bump to 0.12 and run your existing tests, I'd
>    appreciate a heads-up either way -- whether it's a no-op for you or
>    something worth flagging.
> 2. Is it okay to publicly list rmqtt/rmqtt-storage as a downstream user
>    in the project's docs? Happy to link back to your repos, or to leave
>    you out entirely if you'd rather not be named -- your call.
> 3. If you've hit any friction with the crate -- awkward API, missing
>    capability, something you had to work around -- I'd genuinely like to
>    know, even if it's minor. A short template is here if useful:
>    [link to usage-report template], but free-form is fine too.
>
> No obligation to respond quickly or at all -- just wanted the door open.
> Thanks for building on this.

### Draft message: GovCraft/Acton (`Govcraft/acton-service`, maintainer `rrrodzilla`)

> Subject: tower-resilience circuit breaker + bulkhead integration -- quick check-in
>
> Hi -- I maintain [tower-resilience](https://github.com/joshrotenberg/tower-resilience).
> `acton-service` uses both the circuit breaker and bulkhead layers
> (`acton-service/src/middleware/resilience.rs`) for inbound axum
> middleware, including the 5xx-as-failure classifier and the
> `build_with_handle()` pattern for surviving axum's per-request
> `Layer::layer` re-invocation. That's a clean, correct use of both
> crates -- wanted to say so directly.
>
> A few things:
>
> 1. The root Cargo.toml currently pins both circuit breaker and bulkhead
>    to `"0.10.0"`. 0.11 changed circuit-breaker `poll_ready` to check
>    circuit state before inner-service readiness. If you get a chance to
>    bump to 0.12 and run your test suite, I'd appreciate knowing either
>    way -- clean bump or something worth flagging.
> 2. Is it okay to publicly list acton-service as a downstream user in the
>    project's docs? Happy to link back, or leave you out if you'd rather
>    not be named.
> 3. Any friction with either crate -- especially anything the two-layer-builder
>    (outbound vs. `http_circuit_breaker_layer`) pattern had to work around --
>    I'd like to hear about it. Short template here if useful:
>    [link to usage-report template], free-form is also fine.
>
> No obligation to respond quickly or at all. Thanks for building on this.

### Next steps (for the maintainer, not automated)

1. Read and edit the two draft messages above to taste.
2. Decide the channel per project (new issue vs. comment on an existing
   thread) and confirm you're comfortable with the public-paper-trail
   approach.
3. Send. Track responses by appending a "Responses" subsection to this file
   with each maintainer's answer (bump confirmed / friction reported /
   public-listing consent), so the record stays durable across sessions.
4. If either maintainer reports friction, file an issue on
   `joshrotenberg/tower-resilience` describing it (not on their repo) and
   link it back into this doc.
