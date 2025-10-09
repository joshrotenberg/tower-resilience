# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-core-v0.3.0...tower-resilience-core-v0.4.0) - 2025-10-09

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-core-v0.2.1...tower-resilience-core-v0.3.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))

### Other

- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.2.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-core-v0.2.0...tower-resilience-core-v0.2.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-core-v0.1.0...tower-resilience-core-v0.2.0) - 2025-10-08

### Added

- add panic handling to event system and comprehensive tests ([#42](https://github.com/joshrotenberg/tower-resilience/pull/42))
- circuit breaker event system and slow call detection ([#14](https://github.com/joshrotenberg/tower-resilience/pull/14))
- initial tower-resilience workspace with circuitbreaker and bulkhead

### Other

- consolidate all tests to workspace-level tests/ directory ([#48](https://github.com/joshrotenberg/tower-resilience/pull/48))
