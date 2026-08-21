# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking:** `OutlierDetectionConfigBuilder::build` now returns
  `Result<_, OutlierDetectionConfigError>` and rejects a missing detector at
  construction time.
- **Breaking:** `OutlierDetectionService::Future` is now a named,
  `pin_project!`-based `OutlierDetectionFuture<F, C>` instead of
  `BoxFuture<'static, Result<...>>`. This removes one `Box::pin` heap
  allocation per call and, as a direct consequence, drops the `S: Send +
  'static`, `S::Future: Send + 'static`, `Req: Send + 'static`, and `C:
  Send + 'static` bounds from the `Service` impl -- only `S: Service<Req>
  + Clone` and `C: FailureClassifier<S::Response, S::Error> + Clone` are
  required now. Code that names the concrete future type (rare; most
  callers only `.await` it) needs to update to the new type. See
  `docs/tower-api-surface-audit.md` for the allocation-count evidence and
  the crate-by-crate follow-up plan (#426).

### Added

- `OutlierDetectionService::get_ref()`, `get_mut()`, and `into_inner()`
  accessors for the wrapped service, matching Tower's own middleware
  convention.
- Real `metrics`/`tracing` instrumentation: `outlier_ejections_total`,
  `outlier_recoveries_total`, `outlier_ejection_skipped_total`, and
  `outlier_ejected_instances`, plus `tracing::warn!`/`info!`/`debug!` at
  ejection, recovery, and skipped-ejection
  ([#428](https://github.com/joshrotenberg/tower-resilience/issues/428)).

### Removed

- Drop an unused non-dev `tokio` dependency; nothing in the crate's
  library code used it (the only `tokio::` references were in test code,
  already covered by the dev-dependency).
- Drop the `futures` dependency; no longer needed now that
  `Service::Future` is not `BoxFuture`-based.

## [0.12.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.11.0...tower-resilience-outlier-v0.12.0) - 2026-08-17

### Fixed

- *(hedge)* cancel losing attempts and gate eligibility ([#393](https://github.com/joshrotenberg/tower-resilience/pull/393))
- *(reconnect)* rebuild failed services from factory ([#391](https://github.com/joshrotenberg/tower-resilience/pull/391))

## [0.10.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.10.0...tower-resilience-outlier-v0.10.1) - 2026-07-27

### Added

- *(circuitbreaker,outlier)* add failure_classifier_type builder method ([#353](https://github.com/joshrotenberg/tower-resilience/pull/353))

## [0.10.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.9.4...tower-resilience-outlier-v0.10.0) - 2026-06-10

### Added

- add prelude module, make README Quick Start a doctest (closes #310) ([#324](https://github.com/joshrotenberg/tower-resilience/pull/324))

### Fixed

- validate and correct MSRV, add MSRV CI job (closes #312) ([#341](https://github.com/joshrotenberg/tower-resilience/pull/341))
- [**breaking**] return Result from CacheLayer/SharedCacheLayer build() (closes #314) ([#326](https://github.com/joshrotenberg/tower-resilience/pull/326))

## [0.9.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.9.3...tower-resilience-outlier-v0.9.4) - 2026-05-13

### Other

- compose each layer with tower::limit::ConcurrencyLimit ([#302](https://github.com/joshrotenberg/tower-resilience/pull/302))
- share StatefulInner contract probe across layer crates ([#300](https://github.com/joshrotenberg/tower-resilience/pull/300))
- forbid clone-in-call anti-pattern ([#298](https://github.com/joshrotenberg/tower-resilience/pull/298))

## [0.9.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.9.1...tower-resilience-outlier-v0.9.2) - 2026-03-08

### Added

- *(timelimiter,hedge)* add preset configurations ([#269](https://github.com/joshrotenberg/tower-resilience/pull/269))

### Other

- remove redundant About section from README ([#273](https://github.com/joshrotenberg/tower-resilience/pull/273))
- link pattern names to their example sections in README ([#272](https://github.com/joshrotenberg/tower-resilience/pull/272))
- standardize all pattern lists to alphabetical order ([#271](https://github.com/joshrotenberg/tower-resilience/pull/271))

## [0.9.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-outlier-v0.9.0...tower-resilience-outlier-v0.9.1) - 2026-03-08

### Other

- add integration tests for healthcheck and executor ([#265](https://github.com/joshrotenberg/tower-resilience/pull/265))
