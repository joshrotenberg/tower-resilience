# Contributing to tower-resilience

Thank you for your interest in contributing to tower-resilience!

## Getting Started

This is a Cargo workspace with multiple crates. To build and test:

```bash
# Build all crates
cargo build --locked --workspace

# Run all tests
cargo test --locked --workspace --all-features

# Run Clippy across every workspace target (matches CI)
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Check formatting (matches CI)
cargo fmt --all -- --check
```

### Authoritative pre-push checks

Run this complete set before opening a pull request. These commands mirror the
CI gates; do not omit `--workspace`, since that would skip linting package-local
test and example targets.

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --examples --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps
rustup run 1.85.0 cargo check --locked -p tower-resilience --all-features
cargo machete --with-metadata
cargo audit
python3 scripts/public_api.py check
```

`Cargo.lock` is committed for reproducible workspace, example, and security
checks. Include intentional compatible dependency updates in your pull request
and verify the resulting lockfile with the commands above.

Install the dependency-hygiene tool with `cargo install cargo-machete --locked
--version 0.9.2`. CI treats findings as errors. Before removing a reported
dependency, check generated code, build scripts, doctests, optional feature
forwarding, and minimal-feature builds. Document a confirmed false positive in
the owning manifest's `[package.metadata.cargo-machete]` table with a nearby
reason; do not add workspace-wide exceptions. The current audit and its
classifications are recorded in
[`docs/dependency-hygiene.md`](docs/dependency-hygiene.md).

Public API changes must update the checked-in snapshots in the same pull
request after their compatibility impact is reviewed. Install the pinned
nightly and `cargo-public-api`, then run `python3 scripts/public_api.py check`.
For an intentional change, update migration/release notes and run
`python3 scripts/public_api.py update`. The complete procedure, including the
published-release diff command, is in
[`docs/public-api-review.md`](docs/public-api-review.md).

Nightly stress tests are opt-in locally but gating in their GitHub Actions
workflow:

```bash
cargo test --locked --test stress -- --ignored --nocapture --test-threads=1
```

## Running Examples

The project has two sets of examples:

### Top-Level Examples

Simple, getting-started examples in the `examples/` directory:

```bash
cargo run --example circuitbreaker
cargo run --example bulkhead
cargo run --example retry
cargo run --example ratelimiter
cargo run --example timelimiter
cargo run --example cache
cargo run --example chaos
cargo run --example reconnect
cargo run --example adaptive
```

### Module-Specific Examples

Detailed examples in each crate's `examples/` directory showing advanced features:

```bash
# Circuit breaker examples
cargo run --example circuitbreaker_example -p tower-resilience-circuitbreaker
cargo run --example circuitbreaker_fallback -p tower-resilience-circuitbreaker
cargo run --example circuitbreaker_health_check -p tower-resilience-circuitbreaker

# Bulkhead examples
cargo run --example bulkhead_advanced -p tower-resilience-bulkhead
cargo run --example bulkhead_basic -p tower-resilience-bulkhead

# Reconnect examples
cargo run --example reconnect_basic -p tower-resilience-reconnect
cargo run --example reconnect_custom_policy -p tower-resilience-reconnect

# Other pattern examples
cargo run --example cache_example -p tower-resilience-cache
cargo run --example retry_example -p tower-resilience-retry
cargo run --example ratelimiter_example -p tower-resilience-ratelimiter
cargo run --example timelimiter_example -p tower-resilience-timelimiter
cargo run --example chaos_example -p tower-resilience-chaos

# Meta-crate examples (pattern composition, via the facade crate)
cargo run --example combined -p tower-resilience

# Composition and framework-integration examples (workspace root)
cargo run --example composition_outbound
cargo run --example server_api
cargo run --example healthcheck_circuitbreaker
```

## Project Structure

- `crates/tower-resilience-core` - Shared infrastructure (events, metrics)
- `crates/tower-resilience-circuitbreaker` - Circuit breaker pattern
- `crates/tower-resilience-bulkhead` - Bulkhead pattern
- `crates/tower-resilience-timelimiter` - Timeout handling
- `crates/tower-resilience-retry` - Retry with advanced backoff
- `crates/tower-resilience-cache` - Response caching
- `crates/tower-resilience-ratelimiter` - Rate limiting
- `crates/tower-resilience-executor` - Executor delegation
- `crates/tower-resilience-adaptive` - Adaptive concurrency limiting
- `crates/tower-resilience-coalesce` - Request coalescing (singleflight)
- `crates/tower-resilience` - Meta-crate re-exporting all patterns

## Development Guidelines

### Code Standards

- Published crates use Rust 2021 edition with MSRV 1.85.0
- Root workspace uses Rust 2024 edition for development
- When adding new crates, use `edition = "2021"` in their Cargo.toml
- All public APIs must have doc comments
- Run `cargo fmt` before committing
- Ensure `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` passes
- Maintain test coverage

### Builder pattern

Builders in this workspace use the **consuming** style: each setter takes
`mut self` and returns `Self`, so calls chain fluently and end in `.build()`:

```rust
let layer = CircuitBreakerLayer::builder()
    .name("payments")
    .failure_threshold(5)
    .build();
```

This is deliberate. Although the consuming style differs from the
`&mut Self` builder convention, it is the established idiom across every
pattern crate here. New crates should follow it for consistency.

### Implementing a New `Service`

Every layer in this crate implements [`tower::Service`](https://docs.rs/tower-service/latest/tower_service/trait.Service.html). The trait has a non-obvious contract that, if violated, lets a wrapped middleware panic at runtime. Use this checklist on every new `Service` impl and every PR that touches `call` or `poll_ready`.

#### `Service::call` must move the readied receiver

The caller drove `poll_ready` on the instance held by `&mut self`. That instance -- not a fresh clone -- must be the one that runs `call`. The canonical pattern:

```rust
fn call(&mut self, req: Req) -> Self::Future {
    let clone = self.inner.clone();
    let mut inner = std::mem::replace(&mut self.inner, clone);
    Box::pin(async move { inner.call(req).await })
}
```

**Wrong** (panics for any inner whose `Clone` resets readiness state, including `tower::limit::ConcurrencyLimit`, `tower::buffer::Buffer`, `tower::load_shed::LoadShed`):

```rust
fn call(&mut self, req: Req) -> Self::Future {
    let mut inner = self.inner.clone();          // unreadied clone!
    Box::pin(async move { inner.call(req).await })
}
```

The `contract-lints` CI job greps for this anti-pattern and fails the build.

See: [tower-service docs on cloning inner services](https://docs.rs/tower-service/0.3.3/tower_service/trait.Service.html#be-careful-when-cloning-inner-services), #286.

#### `Clone` must reset every per-instance readiness field

Anything `poll_ready` mutates (`sleep`, `permit`, `acquire_task`, etc.) must be reset to its initial state in `Clone`. Otherwise the fresh clone left on `&mut self` by `mem::replace` retains stale readiness state.

#### `poll_ready` must be safe to call repeatedly between `Ready` and the next `call`

The trait docs are explicit: once `poll_ready` returns `Ready(Ok(()))`, repeated calls must continue to return `Ready` (or `Err`). Don't double-acquire permits or restart timers on the second poll -- guard with `if self.permit.is_some()` or equivalent.

#### `poll_ready` must register the waker on `Pending`

If `poll_ready` returns `Pending`, `cx.waker()` must be registered somewhere that will wake the task when the blocking condition clears. Polling a child future via `cx` is the standard way to do this.

#### `Err` from `poll_ready` is terminal

The contract says `Ready(Err(_))` from `poll_ready` means the service is done and should be discarded. Don't return `Err` for transient conditions (rate limited, circuit open, bulkhead full). Surface those as errors from the future returned by `call` instead.

#### Add a contract regression test

`tests/clone_in_call_contract.rs` wraps each layer around a `StatefulInner` whose `Clone` resets readiness. A new layer should add a `<layer>_drives_readied_instance` case to that suite.

For retry paths, cancellation, wake behavior, admission boundaries, and
response/error preservation, compose the reusable `ServiceProbe` and
`ControlledService` from `tower_resilience_core::testing`. The current coverage
and known gaps are tracked in [`docs/tower-contract-matrix.md`](docs/tower-contract-matrix.md).
When reviewing a new or changed `Service` implementation, paste and complete
the [`Tower service review checklist`](docs/tower-service-review-checklist.md)
in the pull request.

`tests/auto_traits.rs` asserts every layer is `Send + Sync + 'static` when its inner is. New layers should be added there too -- a regression that drops `Sync` (e.g., storing a `Pin<Box<dyn Future + Send>>` field) fails to compile there. See #287.

### Testing

- Unit tests in each crate's `src/` files
- Integration tests in workspace `tests/` directory
- Examples should be runnable and well-documented

#### Running Tests

```bash
# Run all tests
cargo test --locked --workspace --all-features

# Run only library tests
cargo test --locked --workspace --all-features --lib

# Run only integration tests
cargo test --locked --workspace --all-features --test '*'

# Run stress tests (opt-in, marked with #[ignore])
cargo test --locked --test stress -- --ignored --test-threads=1

# Run specific pattern stress tests
cargo test --locked --test stress circuitbreaker -- --ignored --test-threads=1
cargo test --locked --test stress bulkhead -- --ignored --test-threads=1
cargo test --locked --test stress cache -- --ignored --test-threads=1

# Run with output to see performance metrics
cargo test --locked --test stress -- --ignored --nocapture --test-threads=1
```

#### Stress Tests

Stress tests validate pattern behavior under extreme conditions:
- High volume (millions of operations)
- High concurrency (thousands of concurrent requests)
- Memory stability (leak detection, bounded growth)
- State consistency (correctness under load)

These tests are marked with `#[ignore]` and must be explicitly run using the `--ignored` flag.

### Commit Messages

Use conventional commit format:
```
<type>: <description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

### Pull Requests

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Ensure all tests pass
6. Submit a pull request

## Release Process

Before cutting a release, review the [upstream Tower crossover
watchlist](docs/upstream-watchlist.md) and update any row whose upstream or
local status has changed since the last review.

Releases are automated by [release-plz](https://release-plz.dev) via
`.github/workflows/release-plz.yml`, which runs on every push to `main` as
two independent jobs:

- `release-plz-release` publishes any packages whose `Cargo.toml` version has
  already been bumped on `main` (crates.io, git tags, GitHub releases).
- `release-plz-pr` opens or updates the PR that prepares the *next* release
  (version bumps + changelog entries), serialized with a `concurrency` group
  so overlapping pushes to `main` queue instead of racing each other into
  opening duplicate release PRs.

All 18 publishable crates under `crates/` use release-plz's default
per-crate changelog path (`crates/<name>/CHANGELOG.md`). `release-plz.toml`
sets `changelog_path` explicitly for `tower-resilience-core`,
`tower-resilience-circuitbreaker`, and `tower-resilience-bulkhead` for
clarity only; every other crate relies on the default, which resolves to the
same location. If a crate is added, confirm its changelog lands at that
default path (or add an explicit `[[package]]` entry if it needs a
different one).

Before merging a release PR, run the complete non-publishing package check:

```bash
python3 scripts/publish_preflight.py
```

See the [release preflight and recovery runbook](docs/release-process.md) for
what the command validates, the publication order, the known release-plz dry
run limitation, monitored-publish steps, and partial-failure recovery.

### Runbook: checking for stale release branches/PRs

Run this check whenever the release automation looks off (e.g. a release PR
sits open longer than expected, or `main` gets multiple release-plz pushes
in quick succession), and periodically as part of release hygiene:

1. List open release-plz PRs:

   ```bash
   gh pr list --search "head:release-plz-" --state open
   ```

   At most one should be open at a time -- it represents the one active
   release train. If more than one is open, or an open one targets a
   version that is already published (check `gh release list` for a
   matching tag, or query crates.io), the extra PR is stale.

2. Close a stale PR with a factual explanatory comment (state why: the
   version is already published and on `main` via another PR), and do
   **not** delete its branch unless you've confirmed it's fully merged or
   the changes it contains are already obsolete. See PR #355 for the
   precedent (closed as a stale duplicate of the v0.10.1 release already
   published and merged via PR #345, tracked in #377).

3. Check for release-plz branches with no corresponding open PR:

   ```bash
   git branch -r --list 'origin/release-plz-*'
   ```

   Delete only branches whose PR is merged or closed and whose commits are
   already fully contained in `main`.

## Questions?

Feel free to open an issue for questions or discussions.
