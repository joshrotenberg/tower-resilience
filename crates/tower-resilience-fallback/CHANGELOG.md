# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.0](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.9.4...tower-resilience-fallback-v0.10.0) - 2026-06-10

### Other

- update Cargo.toml dependencies

## [0.9.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.9.3...tower-resilience-fallback-v0.9.4) - 2026-05-13

### Fixed

- extend clone-in-call fix to fallback, retry, executor ([#297](https://github.com/joshrotenberg/tower-resilience/pull/297))

### Other

- compose each layer with tower::limit::ConcurrencyLimit ([#302](https://github.com/joshrotenberg/tower-resilience/pull/302))
- share StatefulInner contract probe across layer crates ([#300](https://github.com/joshrotenberg/tower-resilience/pull/300))

## [0.7.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.1.4...tower-resilience-fallback-v0.7.1) - 2026-01-29

### Added

- [**breaking**] unify all crates to workspace versioning at 0.7.0 ([#221](https://github.com/joshrotenberg/tower-resilience/pull/221))

### Fixed

- add value_fn() to FallbackLayer for non-Clone response types ([#209](https://github.com/joshrotenberg/tower-resilience/pull/209))

### Other

- add feature flag documentation and standardize imports ([#223](https://github.com/joshrotenberg/tower-resilience/pull/223))

## [0.1.4](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.1.3...tower-resilience-fallback-v0.1.4) - 2026-01-29

### Other

- updated the following local packages: tower-resilience-core

## [0.1.3](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.1.2...tower-resilience-fallback-v0.1.3) - 2026-01-02

### Other

- updated the following local packages: tower-resilience-core

## [0.1.2](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.1.1...tower-resilience-fallback-v0.1.2) - 2026-01-01

### Other

- updated the following local packages: tower-resilience-core

## [0.1.1](https://github.com/joshrotenberg/tower-resilience/compare/tower-resilience-fallback-v0.1.0...tower-resilience-fallback-v0.1.1) - 2026-01-01

### Other

- updated the following local packages: tower-resilience-core
