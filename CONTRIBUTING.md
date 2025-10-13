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

```bash
# Run top-level examples
cargo run --example circuitbreaker
cargo run --example bulkhead
cargo run --example retry

# Run module-specific examples
cargo run --example circuitbreaker_example -p tower-resilience-circuitbreaker
```

## Project Structure

- `crates/tower-resilience-core` - Shared infrastructure (events, metrics)
- `crates/tower-resilience-circuitbreaker` - Circuit breaker pattern
- `crates/tower-resilience-bulkhead` - Bulkhead pattern
- `crates/tower-resilience-timelimiter` - Timeout handling
- `crates/tower-resilience-retry` - Retry with advanced backoff
- `crates/tower-resilience-cache` - Response caching
- `crates/tower-resilience-ratelimiter` - Rate limiting
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
