# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.6](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.4.5...tower-resilience-bulkhead-v0.4.6) - 2025-10-25

### Added

- add tower-resilience-healthcheck module ([#145](https://github.com/joshrotenberg/tower-resilience/pull/145))

### Other

- add reconnect pattern to README and create top-level example ([#142](https://github.com/joshrotenberg/tower-resilience/pull/142))

## [0.4.5](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.4.4...tower-resilience-bulkhead-v0.4.5) - 2025-10-22

### Added

- add circuit breaker Layer integration ([#127](https://github.com/joshrotenberg/tower-resilience/pull/127))

### Other

- add fallback pattern documentation for all resilience layers ([#136](https://github.com/joshrotenberg/tower-resilience/pull/136))

## [0.4.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.4.3...tower-resilience-bulkhead-v0.4.4) - 2025-10-13

### Added

- add chaos engineering layer for testing resilience patterns ([#118](https://github.com/joshrotenberg/tower-resilience/pull/118))

### Other

- add crate metadata for better discoverability ([#120](https://github.com/joshrotenberg/tower-resilience/pull/120))

## [0.4.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.4.2...tower-resilience-bulkhead-v0.4.3) - 2025-10-10

### Added

- add Clone to CircuitBreaker and comprehensive stress tests ([#116](https://github.com/joshrotenberg/tower-resilience/pull/116))

### Other

- improve README examples with event listeners and links ([#114](https://github.com/joshrotenberg/tower-resilience/pull/114))

## [0.4.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.4.0...tower-resilience-bulkhead-v0.4.1) - 2025-10-09

### Added

- *(bulkhead)* add call duration and wait time histograms ([#97](https://github.com/joshrotenberg/tower-resilience/pull/97))

## [0.4.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.3.0...tower-resilience-bulkhead-v0.4.0) - 2025-10-09

### Fixed

- update doctests to use Layer::builder() instead of Config::builder() ([#87](https://github.com/joshrotenberg/tower-resilience/pull/87))

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.2.1...tower-resilience-bulkhead-v0.3.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))

### Other

- add comprehensive event listener callback documentation ([#79](https://github.com/joshrotenberg/tower-resilience/pull/79))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.2.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-bulkhead-v0.2.0...tower-resilience-bulkhead-v0.2.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-bulkhead-v0.1.0...tower-bulkhead-v0.2.0) - 2025-10-08

### Added

- Add bulkhead examples and validate completeness ([#35](https://github.com/joshrotenberg/tower-resilience/pull/35))
- circuit breaker event system and slow call detection ([#14](https://github.com/joshrotenberg/tower-resilience/pull/14))
- initial tower-resilience workspace with circuitbreaker and bulkhead

### Other

- enhance bulkhead documentation with comprehensive examples ([#18](https://github.com/joshrotenberg/tower-resilience/pull/18))
