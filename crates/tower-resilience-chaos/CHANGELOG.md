# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
