# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-ratelimiter-v0.4.0...tower-resilience-ratelimiter-v0.4.1) - 2025-10-09

### Added

- *(ratelimiter)* add comprehensive metrics and tracing support ([#94](https://github.com/joshrotenberg/tower-resilience/pull/94))

## [0.4.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-ratelimiter-v0.3.0...tower-resilience-ratelimiter-v0.4.0) - 2025-10-09

### Other

- [**breaking**] remove Config::builder(), standardize on Layer::builder() API ([#86](https://github.com/joshrotenberg/tower-resilience/pull/86))

## [0.3.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-ratelimiter-v0.2.1...tower-resilience-ratelimiter-v0.3.0) - 2025-10-09

### Added

- add ResilienceError for zero-boilerplate error handling ([#80](https://github.com/joshrotenberg/tower-resilience/pull/80))
- [**breaking**] standardize builder API to return Layer from build() ([#77](https://github.com/joshrotenberg/tower-resilience/pull/77))

### Other

- add comprehensive event listener callback documentation ([#79](https://github.com/joshrotenberg/tower-resilience/pull/79))
- add comprehensive layer composition guide and update builder API examples ([#78](https://github.com/joshrotenberg/tower-resilience/pull/78))

## [0.2.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-ratelimiter-v0.2.0...tower-resilience-ratelimiter-v0.2.1) - 2025-10-08

### Fixed

- add README.md to all published crates ([#68](https://github.com/joshrotenberg/tower-resilience/pull/68))

### Other

- update MSRV to 1.64.0 to match Tower ([#65](https://github.com/joshrotenberg/tower-resilience/pull/65))

## [0.2.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-ratelimiter-plus-v0.1.0...tower-ratelimiter-plus-v0.2.0) - 2025-10-08

### Added

- Add tower-ratelimiter-plus with Resilience4j-inspired rate limiting ([#34](https://github.com/joshrotenberg/tower-resilience/pull/34))
