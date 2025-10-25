# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
