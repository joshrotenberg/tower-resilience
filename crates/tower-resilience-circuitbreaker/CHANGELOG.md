# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
