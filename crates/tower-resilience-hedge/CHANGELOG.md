# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.12.0...tower-resilience-hedge-v0.13.0) - 2026-08-21

### Added

- *(observability)* implement metrics/tracing for adaptive, executor, hedge, outlier, reconnect (closes #428) ([#432](https://github.com/joshrotenberg/tower-resilience/pull/432))

### Fixed

- compose generic errors with Tower BoxError ([#445](https://github.com/joshrotenberg/tower-resilience/pull/445))

### Other

- clean unused workspace dependencies ([#453](https://github.com/joshrotenberg/tower-resilience/pull/453))
- add workspace publish preflight ([#452](https://github.com/joshrotenberg/tower-resilience/pull/452))
- consolidate docs/ around audience and reduce drift surface (closes #439) ([#440](https://github.com/joshrotenberg/tower-resilience/pull/440))
- reconcile README, rustdoc, and examples with verified behavior (closes #380) ([#429](https://github.com/joshrotenberg/tower-resilience/pull/429))
- audit Tower API surface for genericity and runtime coupling (closes #376) ([#424](https://github.com/joshrotenberg/tower-resilience/pull/424))

### Added

- `Hedge::get_ref()`, `get_mut()`, and `into_inner()` accessors for the
  wrapped service, matching Tower's own middleware convention.
- Real `metrics`/`tracing` instrumentation: `hedge_attempts_total`,
  `hedge_calls_total{result}`, and `hedge_call_duration_seconds`, plus
  `tracing::debug!`/`warn!` at hedge starts and terminal outcomes
  ([#428](https://github.com/joshrotenberg/tower-resilience/issues/428)).

## [0.12.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.11.0...tower-resilience-hedge-v0.12.0) - 2026-08-17

### Fixed

- *(hedge)* cancel losing attempts and gate eligibility ([#393](https://github.com/joshrotenberg/tower-resilience/pull/393))

### Added

- Add a per-request eligibility predicate so non-idempotent requests execute once.

### Fixed

- Cancel losing and caller-abandoned attempt futures instead of detaching spawned tasks.
- Wait for every failed attempt and consistently return the primary error.

## [0.10.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.9.4...tower-resilience-hedge-v0.10.0) - 2026-06-10

### Other

- update Cargo.toml dependencies

## [0.9.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.9.3...tower-resilience-hedge-v0.9.4) - 2026-05-13

### Other

- update Cargo.toml dependencies

## [0.9.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.9.1...tower-resilience-hedge-v0.9.2) - 2026-03-08

### Added

- *(timelimiter,hedge)* add preset configurations ([#269](https://github.com/joshrotenberg/tower-resilience/pull/269))

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.1.4...tower-resilience-hedge-v0.7.1) - 2026-01-29

### Added

- [**breaking**] unify all crates to workspace versioning at 0.7.0 ([#221](https://github.com/joshrotenberg/tower-resilience/pull/221))
- [**breaking**] remove type parameters from ChaosLayer and HedgeLayer ([#203](https://github.com/joshrotenberg/tower-resilience/pull/203)) ([#213](https://github.com/joshrotenberg/tower-resilience/pull/213))

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))
- document Hedge Clone requirements for Req and E types ([#210](https://github.com/joshrotenberg/tower-resilience/pull/210))

## [0.1.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.1.3...tower-resilience-hedge-v0.1.4) - 2026-01-29

### Other

- updated the following local packages: tower-resilience-core

## [0.1.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.1.2...tower-resilience-hedge-v0.1.3) - 2026-01-02

### Other

- updated the following local packages: tower-resilience-core

## [0.1.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.1.1...tower-resilience-hedge-v0.1.2) - 2026-01-01

### Other

- updated the following local packages: tower-resilience-core

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-hedge-v0.1.0...tower-resilience-hedge-v0.1.1) - 2026-01-01

### Other

- updated the following local packages: tower-resilience-core
