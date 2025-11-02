# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.8](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.7...tower-resilience-v0.3.8) - 2025-11-02

### Other

- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- improve documentation and examples for new contributors ([#147](https://github.com/joshrotenberg/tower-resilience/pull/147))

## [0.3.7](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.6...tower-resilience-v0.3.7) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))

### Other

- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))

## [0.3.6](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.5...tower-resilience-v0.3.6) - 2025-10-25

### Added

- add reconnect layer with configurable backoff strategies ([#140](https://github.com/joshrotenberg/tower-resilience/pull/140))

## [0.3.5](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.4...tower-resilience-v0.3.5) - 2025-10-22

### Added

- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))

## [0.3.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.3...tower-resilience-v0.3.4) - 2025-10-13

### Added

- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))

## [0.3.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.2...tower-resilience-v0.3.3) - 2025-10-10

### Added

- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))

## [0.3.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.1...tower-resilience-v0.3.2) - 2025-10-10

### Other

- updated the following local packages: tower-resilience-core, tower-resilience-circuitbreaker, tower-resilience-bulkhead, tower-resilience-retry, tower-resilience-cache, tower-resilience-ratelimiter, tower-resilience-timelimiter

## [0.3.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.3.0...tower-resilience-v0.3.1) - 2025-10-09

### Other

- add comprehensive metrics documentation and example ([#100](https://github.com/joshrotenberg/tower-resilience/pull/100))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.2.0...tower-resilience-v0.3.0) - 2025-10-09

### Fixed

- update doctests to use Layer::builder() instead of Config::builder() ([#87](https://github.com/joshrotenberg/tower-resilience/pull/87))

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.1.1...tower-resilience-v0.2.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- [**breaking**] standardize builder API to return Layer from build() ([#77](https://github.com/joshrotenberg/tower-resilience/pull/77))

### Other

- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-v0.1.0...tower-resilience-v0.1.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- add comprehensive pattern guides and use case examples ([#69](https://github.com/joshrotenberg/tower-resilience/pull/69))

## [0.1.0](https://github.com/joshrotenberg/tower-resilience/releases/tag/tower-resilience-v0.1.0) - 2025-10-08

### Added

- update meta-crate with all resilience patterns ([#33](https://github.com/joshrotenberg/tower-resilience/pull/33))
- circuit breaker event system and slow call detection ([#14](https://github.com/joshrotenberg/tower-resilience/pull/14))
