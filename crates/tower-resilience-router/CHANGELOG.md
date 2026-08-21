# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.12.0...tower-resilience-router-v0.13.0) - 2026-08-21

### Added

- *(observability)* implement metrics/tracing for adaptive, executor, hedge, outlier, reconnect (closes #428) ([#432](https://github.com/joshrotenberg/tower-resilience/pull/432))
- *(fallback)* support generic backup services ([#413](https://github.com/joshrotenberg/tower-resilience/pull/413))

### Fixed

- compose generic errors with Tower BoxError ([#445](https://github.com/joshrotenberg/tower-resilience/pull/445))
- *(router)* preserve distribution across clones ([#401](https://github.com/joshrotenberg/tower-resilience/pull/401))
- *(timelimiter)* define streamed HTTP timeout phases ([#403](https://github.com/joshrotenberg/tower-resilience/pull/403))

### Other

- consolidate docs/ around audience and reduce drift surface (closes #439) ([#440](https://github.com/joshrotenberg/tower-resilience/pull/440))
- reconcile README, rustdoc, and examples with verified behavior (closes #380) ([#429](https://github.com/joshrotenberg/tower-resilience/pull/429))
- audit Tower API surface for genericity and runtime coupling (closes #376) ([#424](https://github.com/joshrotenberg/tower-resilience/pull/424))
- audit and consolidate examples around real tower-resilience usage (closes #388) ([#420](https://github.com/joshrotenberg/tower-resilience/pull/420))
- *(circuitbreaker)* validate circuit breaker against tower-rs/tower#855 (closes #375) ([#418](https://github.com/joshrotenberg/tower-resilience/pull/418))

### Fixed

- share smooth weighted-round-robin progression across router clones, including clone-per-request and concurrent traffic ([#366](https://github.com/joshrotenberg/tower-resilience/issues/366))

### Removed

- Drop an unused non-dev `tokio` dependency; nothing in the crate's
  library code used it (the only `tokio::` references were in test code,
  already covered by the dev-dependency).

## [0.12.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.11.0...tower-resilience-router-v0.12.0) - 2026-08-17

### Fixed

- *(hedge)* cancel losing attempts and gate eligibility ([#393](https://github.com/joshrotenberg/tower-resilience/pull/393))
- *(reconnect)* rebuild failed services from factory ([#391](https://github.com/joshrotenberg/tower-resilience/pull/391))

## [0.10.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.9.4...tower-resilience-router-v0.10.0) - 2026-06-10

### Added

- add prelude module, make README Quick Start a doctest (closes #310) ([#324](https://github.com/joshrotenberg/tower-resilience/pull/324))

### Fixed

- validate and correct MSRV, add MSRV CI job (closes #312) ([#341](https://github.com/joshrotenberg/tower-resilience/pull/341))
- [**breaking**] return Result from CacheLayer/SharedCacheLayer build() (closes #314) ([#326](https://github.com/joshrotenberg/tower-resilience/pull/326))

## [0.9.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.9.3...tower-resilience-router-v0.9.4) - 2026-05-13

### Other

- update Cargo.toml dependencies

## [0.9.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.9.1...tower-resilience-router-v0.9.2) - 2026-03-08

### Added

- *(timelimiter,hedge)* add preset configurations ([#269](https://github.com/joshrotenberg/tower-resilience/pull/269))

### Other

- remove redundant About section from README ([#273](https://github.com/joshrotenberg/tower-resilience/pull/273))
- link pattern names to their example sections in README ([#272](https://github.com/joshrotenberg/tower-resilience/pull/272))
- standardize all pattern lists to alphabetical order ([#271](https://github.com/joshrotenberg/tower-resilience/pull/271))

## [0.9.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-router-v0.9.0...tower-resilience-router-v0.9.1) - 2026-03-08

### Other

- add integration tests for healthcheck and executor ([#265](https://github.com/joshrotenberg/tower-resilience/pull/265))
