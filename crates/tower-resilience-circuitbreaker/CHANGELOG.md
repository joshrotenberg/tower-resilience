# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.7.0...tower-resilience-circuitbreaker-v0.7.1) - 2026-02-03

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))

## [0.6.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.5.2...tower-resilience-circuitbreaker-v0.6.0) - 2026-01-29

### Other

- [**breaking**] change BulkheadLayer::max_wait_duration to accept Duration ([#193](https://github.com/joshrotenberg/tower-resilience/pull/193))

## [0.5.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.5.1...tower-resilience-circuitbreaker-v0.5.2) - 2026-01-02

### Fixed

- bump tower-resilience to 0.4.0 for breaking sub-crate changes ([#187](https://github.com/joshrotenberg/tower-resilience/pull/187))

## [0.5.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.5.0...tower-resilience-circuitbreaker-v0.5.1) - 2026-01-01

### Other

- add comprehensive pattern selection and composition guide ([#185](https://github.com/joshrotenberg/tower-resilience/pull/185))

## [0.5.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.7...tower-resilience-circuitbreaker-v0.5.0) - 2026-01-01

### Added

- add request coalescing (singleflight) pattern ([#180](https://github.com/joshrotenberg/tower-resilience/pull/180))
- add adaptive concurrency limiter with AIMD and Vegas algorithms ([#178](https://github.com/joshrotenberg/tower-resilience/pull/178))
- add executor delegation layer for parallel request processing ([#177](https://github.com/joshrotenberg/tower-resilience/pull/177))
- add per-request max attempts extraction to retry ([#176](https://github.com/joshrotenberg/tower-resilience/pull/176))

### Fixed

- resolve example name collision and standardize naming ([#174](https://github.com/joshrotenberg/tower-resilience/pull/174))

### Other

- simplify README and reduce marketing language ([#173](https://github.com/joshrotenberg/tower-resilience/pull/173))

## [0.4.7](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.6...tower-resilience-circuitbreaker-v0.4.7) - 2025-11-02

### Other

- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- improve documentation and examples for new contributors ([#147](https://github.com/joshrotenberg/tower-resilience/pull/147))

## [0.4.6](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.5...tower-resilience-circuitbreaker-v0.4.6) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))

### Other

- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))

## [0.4.5](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.4...tower-resilience-circuitbreaker-v0.4.5) - 2025-10-22

### Added

- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))

### Other

- add fallback pattern documentation for all resilience layers ([#136](https://github.com/joshrotenberg/tower-resilience/pull/136))

## [0.4.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.3...tower-resilience-circuitbreaker-v0.4.4) - 2025-10-13

### Added

- add health check integration for circuit breaker ([#106](https://github.com/joshrotenberg/tower-resilience/pull/106)) ([#121](https://github.com/joshrotenberg/tower-resilience/pull/121))
- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))

## [0.4.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.2...tower-resilience-circuitbreaker-v0.4.3) - 2025-10-10

### Added

- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))

## [0.4.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.1...tower-resilience-circuitbreaker-v0.4.2) - 2025-10-10

### Added

- add sync state inspection API for circuit breaker ([#112](https://github.com/joshrotenberg/tower-resilience/pull/112))

### Other

- add comprehensive fallback examples and improve documentation ([#109](https://github.com/joshrotenberg/tower-resilience/pull/109))

## [0.4.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.4.0...tower-resilience-circuitbreaker-v0.4.1) - 2025-10-09

### Added

- *(circuitbreaker)* enhance metrics with instance names and slow call tracking ([#96](https://github.com/joshrotenberg/tower-resilience/pull/96))

## [0.4.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.3.0...tower-resilience-circuitbreaker-v0.4.0) - 2025-10-09

### Fixed

- update doctests to use Layer::builder() instead of Config::builder() ([#87](https://github.com/joshrotenberg/tower-resilience/pull/87))

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.2.1...tower-resilience-circuitbreaker-v0.3.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))

### Other

- add comprehensive event listener callback documentation ([#79](https://github.com/joshrotenberg/tower-resilience/pull/79))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.2.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-circuitbreaker-v0.2.0...tower-resilience-circuitbreaker-v0.2.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-circuitbreaker-v0.1.0...tower-circuitbreaker-v0.2.0) - 2025-10-08

### Added

- Add bulkhead examples and validate completeness ([#35](https://github.com/joshrotenberg/tower-resilience/pull/35))
- add time-based sliding window to circuit breaker ([#20](https://github.com/joshrotenberg/tower-resilience/pull/20))
- add sync state inspection to circuit breaker ([#15](https://github.com/joshrotenberg/tower-resilience/pull/15))
- circuit breaker event system and slow call detection ([#14](https://github.com/joshrotenberg/tower-resilience/pull/14))
- initial tower-resilience workspace with circuitbreaker and bulkhead

### Other

- add comprehensive examples to circuit breaker documentation ([#22](https://github.com/joshrotenberg/tower-resilience/pull/22))
- update metrics-util to v0.20 ([#17](https://github.com/joshrotenberg/tower-resilience/pull/17))
