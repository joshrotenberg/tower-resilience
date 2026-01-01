# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.5.0...tower-resilience-chaos-v0.5.1) - 2026-01-01

### Other

- add comprehensive pattern selection and composition guide ([#185](https://github.com/joshrotenberg/tower-resilience/pull/185))

## [0.5.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.4.7...tower-resilience-chaos-v0.5.0) - 2026-01-01

### Added

- add request coalescing (singleflight) pattern ([#180](https://github.com/joshrotenberg/tower-resilience/pull/180))
- add adaptive concurrency limiter with AIMD and Vegas algorithms ([#178](https://github.com/joshrotenberg/tower-resilience/pull/178))
- add executor delegation layer for parallel request processing ([#177](https://github.com/joshrotenberg/tower-resilience/pull/177))
- add per-request max attempts extraction to retry ([#176](https://github.com/joshrotenberg/tower-resilience/pull/176))

### Fixed

- resolve example name collision and standardize naming ([#174](https://github.com/joshrotenberg/tower-resilience/pull/174))

### Other

- simplify README and reduce marketing language ([#173](https://github.com/joshrotenberg/tower-resilience/pull/173))

## [0.4.7](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.4.6...tower-resilience-chaos-v0.4.7) - 2025-11-02

### Other

- update dependencies and remove unused thiserror ([#149](https://github.com/joshrotenberg/tower-resilience/pull/149))
- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- improve documentation and examples for new contributors ([#147](https://github.com/joshrotenberg/tower-resilience/pull/147))

## [0.4.6](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.4.5...tower-resilience-chaos-v0.4.6) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))

### Other

- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))

## [0.4.5](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.4.4...tower-resilience-chaos-v0.4.5) - 2025-10-22

### Added

- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))

## [0.4.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-chaos-v0.4.3...tower-resilience-chaos-v0.4.4) - 2025-10-13

### Added

- add axum resilient key-value store example ([#122](https://github.com/joshrotenberg/tower-resilience/pull/122))

### Added
- Initial release of tower-resilience-chaos
- Error injection with configurable failure rates
- Latency injection with variable delay options
- Event system integration for observability
- Deterministic seeding support for reproducible tests
- Safety guards to prevent production usage
