# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-executor-v0.5.2...tower-resilience-executor-v0.6.0) - 2026-01-29

### Other

- [**breaking**] change BulkheadLayer::max_wait_duration to accept Duration ([#193](https://github.com/joshrotenberg/tower-resilience/pull/193))

## [0.5.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-executor-v0.5.1...tower-resilience-executor-v0.5.2) - 2026-01-02

### Fixed

- bump tower-resilience to 0.4.0 for breaking sub-crate changes ([#187](https://github.com/joshrotenberg/tower-resilience/pull/187))

## [0.5.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-executor-v0.5.0...tower-resilience-executor-v0.5.1) - 2026-01-01

### Other

- add comprehensive pattern selection and composition guide ([#185](https://github.com/joshrotenberg/tower-resilience/pull/185))

## [0.5.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-executor-v0.4.7...tower-resilience-executor-v0.5.0) - 2026-01-01

### Added

- add request coalescing (singleflight) pattern ([#180](https://github.com/joshrotenberg/tower-resilience/pull/180))
- add adaptive concurrency limiter with AIMD and Vegas algorithms ([#178](https://github.com/joshrotenberg/tower-resilience/pull/178))
