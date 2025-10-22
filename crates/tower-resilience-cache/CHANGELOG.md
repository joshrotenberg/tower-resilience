# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.5](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.3.4...tower-resilience-cache-v0.3.5) - 2025-10-22

### Added

- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))

## [0.3.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.3.3...tower-resilience-cache-v0.3.4) - 2025-10-13

### Added

- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))

## [0.3.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.3.2...tower-resilience-cache-v0.3.3) - 2025-10-10

### Added

- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))

## [0.3.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.3.1...tower-resilience-cache-v0.3.2) - 2025-10-10

### Added

- add LFU and FIFO cache eviction policies ([#113](https://github.com/joshrotenberg/tower-resilience/pull/113))

## [0.3.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.3.0...tower-resilience-cache-v0.3.1) - 2025-10-09

### Added

- *(cache)* add comprehensive metrics and tracing support ([#91](https://github.com/joshrotenberg/tower-resilience/pull/91))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.2.0...tower-resilience-cache-v0.3.0) - 2025-10-09

### Fixed

- update doctests to use Layer::builder() instead of Config::builder() ([#87](https://github.com/joshrotenberg/tower-resilience/pull/87))

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.1.1...tower-resilience-cache-v0.2.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- [**breaking**] standardize builder API to return Layer from build() ([#77](https://github.com/joshrotenberg/tower-resilience/pull/77))

### Other

- add comprehensive event listener callback documentation ([#79](https://github.com/joshrotenberg/tower-resilience/pull/79))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-cache-v0.1.0...tower-resilience-cache-v0.1.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- release ([#58](https://github.com/joshrotenberg/tower-resilience/pull/58))
- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))

## [0.1.0](https://github.com/joshrotenberg/tower-resilience/releases/tag/tower-resilience-cache-v0.1.0) - 2025-10-08

### Other

- rename all crates to tower-resilience-* namespace ([#57](https://github.com/joshrotenberg/tower-resilience/pull/57))
