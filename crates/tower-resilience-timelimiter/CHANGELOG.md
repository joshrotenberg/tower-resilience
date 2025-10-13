# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.3.3...tower-resilience-timelimiter-v0.3.4) - 2025-10-13

### Added

- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))

## [0.3.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.3.2...tower-resilience-timelimiter-v0.3.3) - 2025-10-10

### Added

- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))

## [0.3.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.3.1...tower-resilience-timelimiter-v0.3.2) - 2025-10-10

### Other

- updated the following local packages: tower-resilience-core

## [0.3.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.3.0...tower-resilience-timelimiter-v0.3.1) - 2025-10-09

### Added

- *(timelimiter)* add comprehensive metrics and tracing support ([#95](https://github.com/joshrotenberg/tower-resilience/pull/95))
- *(cache)* add comprehensive metrics and tracing support ([#91](https://github.com/joshrotenberg/tower-resilience/pull/91))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.2.0...tower-resilience-timelimiter-v0.3.0) - 2025-10-09

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.1.1...tower-resilience-timelimiter-v0.2.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- [**breaking**] standardize builder API to return Layer from build() ([#77](https://github.com/joshrotenberg/tower-resilience/pull/77))

### Other

- add comprehensive event listener callback documentation ([#79](https://github.com/joshrotenberg/tower-resilience/pull/79))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-timelimiter-v0.1.0...tower-resilience-timelimiter-v0.1.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))

## [0.1.0](https://github.com/joshrotenberg/tower-resilience/releases/tag/tower-timelimiter-v0.1.0) - 2025-10-08

### Added

- add panic handling to event system and comprehensive tests ([#42](https://github.com/joshrotenberg/tower-resilience/pull/42))
- Add bulkhead examples and validate completeness ([#35](https://github.com/joshrotenberg/tower-resilience/pull/35))
- implement tower-timelimiter basic functionality ([#26](https://github.com/joshrotenberg/tower-resilience/pull/26))

### Other

- consolidate all tests to workspace-level tests/ directory ([#48](https://github.com/joshrotenberg/tower-resilience/pull/48))
- add comprehensive tests for tower-cache ([#43](https://github.com/joshrotenberg/tower-resilience/pull/43))
- add comprehensive P0 tests for timelimiter pattern ([#41](https://github.com/joshrotenberg/tower-resilience/pull/41))
