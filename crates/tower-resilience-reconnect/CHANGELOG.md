# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.7.0...tower-resilience-reconnect-v0.7.1) - 2026-01-29

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))

## [0.6.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.5.2...tower-resilience-reconnect-v0.6.0) - 2026-01-29

### Other

- [**breaking**] change BulkheadLayer::max_wait_duration to accept Duration ([#193](https://github.com/joshrotenberg/tower-resilience/pull/193))

## [0.5.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.5.1...tower-resilience-reconnect-v0.5.2) - 2026-01-02

### Fixed

- bump tower-resilience to 0.4.0 for breaking sub-crate changes ([#187](https://github.com/joshrotenberg/tower-resilience/pull/187))

## [0.5.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.5.0...tower-resilience-reconnect-v0.5.1) - 2026-01-01

### Other

- add comprehensive pattern selection and composition guide ([#185](https://github.com/joshrotenberg/tower-resilience/pull/185))

## [0.5.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.4.7...tower-resilience-reconnect-v0.5.0) - 2026-01-01

### Added

- add request coalescing (singleflight) pattern ([#180](https://github.com/joshrotenberg/tower-resilience/pull/180))
- add adaptive concurrency limiter with AIMD and Vegas algorithms ([#178](https://github.com/joshrotenberg/tower-resilience/pull/178))
- add executor delegation layer for parallel request processing ([#177](https://github.com/joshrotenberg/tower-resilience/pull/177))
- add per-request max attempts extraction to retry ([#176](https://github.com/joshrotenberg/tower-resilience/pull/176))

### Fixed

- resolve example name collision and standardize naming ([#174](https://github.com/joshrotenberg/tower-resilience/pull/174))

### Other

- simplify README and reduce marketing language ([#173](https://github.com/joshrotenberg/tower-resilience/pull/173))

## [0.4.7](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.4.6...tower-resilience-reconnect-v0.4.7) - 2025-11-02

### Other

- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- improve documentation and examples for new contributors ([#147](https://github.com/joshrotenberg/tower-resilience/pull/147))

## [0.4.6](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-reconnect-v0.4.5...tower-resilience-reconnect-v0.4.6) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))

### Other

- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))
- release ([#141](https://github.com/joshrotenberg/tower-resilience/pull/141))

## [0.4.5](https://github.com/joshrotenberg/tower-resilience/releases/tag/tower-resilience-reconnect-v0.4.5) - 2025-10-25

### Added

- add reconnect layer with configurable backoff strategies ([#140](https://github.com/joshrotenberg/tower-resilience/pull/140))
- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))
- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))
- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))
- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- add criterion benchmarks for happy path overhead ([#64](https://github.com/joshrotenberg/tower-resilience/pull/64))
- initial tower-resilience workspace with circuitbreaker and bulkhead

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))
- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))
- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))
- rename all crates to tower-resilience-* namespace ([#57](https://github.com/joshrotenberg/tower-resilience/pull/57))
- enhance README with badges, examples, and context ([#52](https://github.com/joshrotenberg/tower-resilience/pull/52))
