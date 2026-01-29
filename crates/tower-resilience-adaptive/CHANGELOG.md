# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-adaptive-v0.6.0...tower-resilience-adaptive-v0.7.1) - 2026-01-29

### Added

- [**breaking**] unify all crates to workspace versioning at 0.7.0 ([#221](https://github.com/joshrotenberg/tower-resilience/pull/221))
- [**breaking**] remove type parameters from ChaosLayer and HedgeLayer ([#203](https://github.com/joshrotenberg/tower-resilience/pull/203)) ([#213](https://github.com/joshrotenberg/tower-resilience/pull/213))
- [**breaking**] simplify CircuitBreakerLayer API with trait-based classifiers ([#199](https://github.com/joshrotenberg/tower-resilience/pull/199))

### Fixed

- [**breaking**] implement/remove dead code identified in audit ([#195](https://github.com/joshrotenberg/tower-resilience/pull/195)) ([#196](https://github.com/joshrotenberg/tower-resilience/pull/196))

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))
- fix version references and example file path ([#218](https://github.com/joshrotenberg/tower-resilience/pull/218))
- add comprehensive ResilienceError documentation for composed layers ([#217](https://github.com/joshrotenberg/tower-resilience/pull/217))
- document preset configurations in README and lib.rs ([#216](https://github.com/joshrotenberg/tower-resilience/pull/216))
- document Hedge Clone requirements for Req and E types ([#210](https://github.com/joshrotenberg/tower-resilience/pull/210))

## [0.6.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-adaptive-v0.5.2...tower-resilience-adaptive-v0.6.0) - 2026-01-29

### Other

- [**breaking**] change BulkheadLayer::max_wait_duration to accept Duration ([#193](https://github.com/joshrotenberg/tower-resilience/pull/193))

## [0.5.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-adaptive-v0.5.1...tower-resilience-adaptive-v0.5.2) - 2026-01-02

### Fixed

- bump tower-resilience to 0.4.0 for breaking sub-crate changes ([#187](https://github.com/joshrotenberg/tower-resilience/pull/187))

## [0.5.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-adaptive-v0.5.0...tower-resilience-adaptive-v0.5.1) - 2026-01-01

### Other

- add comprehensive pattern selection and composition guide ([#185](https://github.com/joshrotenberg/tower-resilience/pull/185))

## [0.5.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-adaptive-v0.4.7...tower-resilience-adaptive-v0.5.0) - 2026-01-01

### Added

- add request coalescing (singleflight) pattern ([#180](https://github.com/joshrotenberg/tower-resilience/pull/180))
- add adaptive concurrency limiter with AIMD and Vegas algorithms ([#178](https://github.com/joshrotenberg/tower-resilience/pull/178))
- add executor delegation layer for parallel request processing ([#177](https://github.com/joshrotenberg/tower-resilience/pull/177))
- add per-request max attempts extraction to retry ([#176](https://github.com/joshrotenberg/tower-resilience/pull/176))
- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))
- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))
- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))
- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))
- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- add criterion benchmarks for happy path overhead ([#64](https://github.com/joshrotenberg/tower-resilience/pull/64))
- initial tower-resilience workspace with circuitbreaker and bulkhead

### Fixed

- resolve example name collision and standardize naming ([#174](https://github.com/joshrotenberg/tower-resilience/pull/174))

### Other

- simplify README and reduce marketing language ([#173](https://github.com/joshrotenberg/tower-resilience/pull/173))
- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- improve documentation and examples for new contributors ([#147](https://github.com/joshrotenberg/tower-resilience/pull/147))
- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))
- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))
- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))
- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))
- rename all crates to tower-resilience-* namespace ([#57](https://github.com/joshrotenberg/tower-resilience/pull/57))
- enhance README with badges, examples, and context ([#52](https://github.com/joshrotenberg/tower-resilience/pull/52))
