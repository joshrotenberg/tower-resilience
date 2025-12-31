# Contributing to tower-resilience

Thank you for your interest in contributing to tower-resilience!

## Getting Started

This is a Cargo workspace with multiple crates. To build and test:

```bash
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace --all-features

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt --all
```

## Running Examples

The project has two sets of examples:

### Top-Level Examples

Simple, getting-started examples in the `examples/` directory:

```bash
cargo run --example circuitbreaker
cargo run --example bulkhead
cargo run --example retry
cargo run --example ratelimiter
cargo run --example timelimiter
cargo run --example cache
cargo run --example chaos
cargo run --example reconnect
```

### Module-Specific Examples

Detailed examples in each crate's `examples/` directory showing advanced features:

```bash
# Circuit breaker examples
cargo run --example circuitbreaker_example -p tower-resilience-circuitbreaker
cargo run --example circuitbreaker_fallback -p tower-resilience-circuitbreaker
cargo run --example circuitbreaker_health_check -p tower-resilience-circuitbreaker

# Bulkhead examples
cargo run --example bulkhead_advanced -p tower-resilience-bulkhead
cargo run --example bulkhead_basic -p tower-resilience-bulkhead

# Reconnect examples
cargo run --example reconnect_basic -p tower-resilience-reconnect
cargo run --example reconnect_custom_policy -p tower-resilience-reconnect

# Other pattern examples
cargo run --example cache_example -p tower-resilience-cache
cargo run --example retry_example -p tower-resilience-retry
cargo run --example ratelimiter_example -p tower-resilience-ratelimiter
cargo run --example timelimiter_example -p tower-resilience-timelimiter
cargo run --example chaos_example -p tower-resilience-chaos

# Meta-crate examples (pattern composition)
cargo run --example full_stack -p tower-resilience
cargo run --example combined -p tower-resilience
```

## Project Structure

- `crates/tower-resilience-core` - Shared infrastructure (events, metrics)
- `crates/tower-resilience-circuitbreaker` - Circuit breaker pattern
- `crates/tower-resilience-bulkhead` - Bulkhead pattern
- `crates/tower-resilience-timelimiter` - Timeout handling
- `crates/tower-resilience-retry` - Retry with advanced backoff
- `crates/tower-resilience-cache` - Response caching
- `crates/tower-resilience-ratelimiter` - Rate limiting
- `crates/tower-resilience-executor` - Executor delegation
- `crates/tower-resilience` - Meta-crate re-exporting all patterns

## Development Guidelines

### Code Standards

- Published crates use Rust 2021 edition (MSRV 1.64.0, matching Tower's MSRV policy)
- Root workspace uses Rust 2024 edition for development
- When adding new crates, use `edition = "2021"` in their Cargo.toml
- All public APIs must have doc comments
- Run `cargo fmt` before committing
- Ensure `cargo clippy` passes with `-D warnings`
- Maintain test coverage

### Testing

- Unit tests in each crate's `src/` files
- Integration tests in workspace `tests/` directory
- Examples should be runnable and well-documented

#### Running Tests

```bash
# Run all tests
cargo test --workspace --all-features

# Run only library tests
cargo test --workspace --all-features --lib

# Run only integration tests
cargo test --workspace --all-features --test '*'

# Run stress tests (opt-in, marked with #[ignore])
cargo test --test stress -- --ignored

# Run specific pattern stress tests
cargo test --test stress circuitbreaker -- --ignored
cargo test --test stress bulkhead -- --ignored
cargo test --test stress cache -- --ignored

# Run with output to see performance metrics
cargo test --test stress -- --ignored --nocapture
```

#### Stress Tests

Stress tests validate pattern behavior under extreme conditions:
- High volume (millions of operations)
- High concurrency (thousands of concurrent requests)
- Memory stability (leak detection, bounded growth)
- State consistency (correctness under load)

These tests are marked with `#[ignore]` and must be explicitly run using the `--ignored` flag.

### Commit Messages

Use conventional commit format:
```
<type>: <description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

### Pull Requests

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Ensure all tests pass
6. Submit a pull request

## Questions?

Feel free to open an issue for questions or discussions.
