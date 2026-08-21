# Public API review

Audience: maintainers reviewing public API changes or preparing a release.

This repository keeps a simplified `cargo-public-api` snapshot for every
publishable library crate in [`public-api/`](public-api/). The snapshots omit
blanket implementations while retaining auto-trait and derived implementations.
Those traits are part of the usable API: for example, this release intentionally
removes `UnwindSafe` and `RefUnwindSafe` from `TokenBucketBudget`.

## Pinned toolchain

Install the exact versions used by CI:

```bash
rustup toolchain install nightly-2026-08-01 --profile minimal
cargo install cargo-public-api --locked --version 0.52.0
```

The wrapper locates `rustc` and `rustdoc` through `rustup`, so it also works
when the `cargo` executable itself was installed by a package manager.

## Pull request gate

Run the same check as CI:

```bash
python3 scripts/public_api.py check
```

Any unacknowledged public API change fails with a unified diff. If a change is
intentional, review its compatibility impact, update migration and release
notes as needed, and then acknowledge it in the same pull request with:

```bash
python3 scripts/public_api.py update
git diff -- docs/public-api
```

Committing an updated snapshot is the explicit maintainer acknowledgement. It
allows intentional pre-1.0 minor-version breaks after review; CI does not
incorrectly impose stable-major compatibility rules on a 0.x release.

The compile fixtures in `tests/public_api_shapes.rs` provide a complementary
source-level check for fragile shapes that a text diff is easy to misread or
does not render, including methods on builder types reached through type
inference: fallible constructor and builder returns, observable `(Layer,
Handle)` returns, facade re-exports, and the named outlier-detection
`Service::Future`.

## Release comparison

For a minor release, compare the working tree with the current published
version for all publishable crates in one command. For example:

```bash
python3 scripts/public_api.py diff 0.12.0
```

Review every added, changed, and removed item. Record intentional source breaks
in the facade migration guide and in the affected crate and facade changelogs.
The release-plz semver result is another signal, not a substitute: the
[release-plz semver-check documentation](https://release-plz.dev/docs/semver-check)
states that its underlying checker does not detect every violation, and a
pre-1.0 minor version is permitted to contain source-breaking changes.

## 0.13 acknowledgement

The complete 0.12.0-to-0.13 working-tree diff was reviewed across all 18
publishable crates. The intentional source breaks are recorded in
`crates/tower-resilience/MIGRATION.md` and the affected changelogs. They are:

- typed construction errors for core AIMD, adaptive AIMD/Vegas, bulkhead,
  circuit breaker, rate limiter, outlier detection, retry budgets/backoff, and
  reconnect random exponential policy construction;
- a named, allocation-free outlier-detection `Service::Future`;
- `Closed` and `NotReady` bulkhead errors;
- removal of automatic unwind-safety traits from `TokenBucketBudget`;
- generic middleware error-source behavior that accepts Tower `BoxError`; and
- the `streaming()` to `detached()` time-limiter naming transition.

The review also covered compatible additions: wrapped-service accessors,
circuit-breaker manual controls and health triggers, the defaulted second
`FallbackError` parameter and generic Tower backup service, the facade
migration module, and new instrumentation APIs. The fallback type's generated
diff looks structural, but its second parameter defaults to the original error
type and its existing same-error accessors remain available. The router's
acquisition of the compiler's `Freeze` auto trait is also compatible.

Review this record and regenerate all snapshots whenever a public API changes.
For each release, update the acknowledgement and baseline version after
comparing against the version currently published on crates.io.
