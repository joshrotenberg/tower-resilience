# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-healthcheck-v0.1.3...tower-resilience-healthcheck-v0.7.1) - 2026-01-29

### Added

- [**breaking**] unify all crates to workspace versioning at 0.7.0 ([#221](https://github.com/joshrotenberg/tower-resilience/pull/221))
- integrate HealthCheck with CircuitBreaker for proactive circuit opening ([#214](https://github.com/joshrotenberg/tower-resilience/pull/214))
- [**breaking**] remove type parameters from ChaosLayer and HedgeLayer ([#203](https://github.com/joshrotenberg/tower-resilience/pull/203)) ([#213](https://github.com/joshrotenberg/tower-resilience/pull/213))

### Fixed

- add version to healthcheck path dependencies for publishing ([#224](https://github.com/joshrotenberg/tower-resilience/pull/224))

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))

## [0.1.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-healthcheck-v0.1.2...tower-resilience-healthcheck-v0.1.3) - 2026-01-29

### Other

- *(deps)* update reqwest requirement from 0.12 to 0.13 ([#190](https://github.com/joshrotenberg/tower-resilience/pull/190))

## [0.1.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-healthcheck-v0.1.1...tower-resilience-healthcheck-v0.1.2) - 2026-01-01

### Fixed

- resolve example name collision and standardize naming ([#174](https://github.com/joshrotenberg/tower-resilience/pull/174))

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-healthcheck-v0.1.0...tower-resilience-healthcheck-v0.1.1) - 2025-11-02

### Other

- add comprehensive stress tests and benchmarks for reconnect and healthcheck ([#148](https://github.com/joshrotenberg/tower-resilience/pull/148))
- release ([#143](https://github.com/joshrotenberg/tower-resilience/pull/143))

## [0.1.0](https://github.com/joshrotenberg/tower-resilience/releases/tag/tower-resilience-healthcheck-v0.1.0) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))
