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
cargo run --example adaptive
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
- `crates/tower-resilience-adaptive` - Adaptive concurrency limiting
- `crates/tower-resilience-coalesce` - Request coalescing (singleflight)
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

### Builder pattern

Builders in this workspace use the **consuming** style: each setter takes
`mut self` and returns `Self`, so calls chain fluently and end in `.build()`:

```rust
let layer = CircuitBreakerLayer::builder()
    .name("payments")
    .failure_threshold(5)
    .build();
```

This is deliberate. Although the consuming style differs from the
`&mut Self` builder convention, it is the established idiom across every
pattern crate here. New crates should follow it for consistency.

### Implementing a New `Service`

Every layer in this crate implements [`tower::Service`](https://docs.rs/tower-service/latest/tower_service/trait.Service.html). The trait has a non-obvious contract that, if violated, lets a wrapped middleware panic at runtime. Use this checklist on every new `Service` impl and every PR that touches `call` or `poll_ready`.

#### `Service::call` must move the readied receiver

The caller drove `poll_ready` on the instance held by `&mut self`. That instance -- not a fresh clone -- must be the one that runs `call`. The canonical pattern:

```rust
fn call(&mut self, req: Req) -> Self::Future {
    let clone = self.inner.clone();
    let mut inner = std::mem::replace(&mut self.inner, clone);
    Box::pin(async move { inner.call(req).await })
}
```

**Wrong** (panics for any inner whose `Clone` resets readiness state, including `tower::limit::ConcurrencyLimit`, `tower::buffer::Buffer`, `tower::load_shed::LoadShed`):

```rust
fn call(&mut self, req: Req) -> Self::Future {
    let mut inner = self.inner.clone();          // unreadied clone!
    Box::pin(async move { inner.call(req).await })
}
```

The `contract-lints` CI job greps for this anti-pattern and fails the build.

See: [tower-service docs on cloning inner services](https://docs.rs/tower-service/0.3.3/tower_service/trait.Service.html#be-careful-when-cloning-inner-services), #286.

#### `Clone` must reset every per-instance readiness field

Anything `poll_ready` mutates (`sleep`, `permit`, `acquire_task`, etc.) must be reset to its initial state in `Clone`. Otherwise the fresh clone left on `&mut self` by `mem::replace` retains stale readiness state.

#### `poll_ready` must be safe to call repeatedly between `Ready` and the next `call`

The trait docs are explicit: once `poll_ready` returns `Ready(Ok(()))`, repeated calls must continue to return `Ready` (or `Err`). Don't double-acquire permits or restart timers on the second poll -- guard with `if self.permit.is_some()` or equivalent.

#### `poll_ready` must register the waker on `Pending`

If `poll_ready` returns `Pending`, `cx.waker()` must be registered somewhere that will wake the task when the blocking condition clears. Polling a child future via `cx` is the standard way to do this.

#### `Err` from `poll_ready` is terminal

The contract says `Ready(Err(_))` from `poll_ready` means the service is done and should be discarded. Don't return `Err` for transient conditions (rate limited, circuit open, bulkhead full). Surface those as errors from the future returned by `call` instead.

#### Add a contract regression test

`tests/clone_in_call_contract.rs` wraps each layer around a `StatefulInner` whose `Clone` resets readiness. A new layer should add a `<layer>_drives_readied_instance` case to that suite.

`tests/auto_traits.rs` asserts every layer is `Send + Sync + 'static` when its inner is. New layers should be added there too -- a regression that drops `Sync` (e.g., storing a `Pin<Box<dyn Future + Send>>` field) fails to compile there. See #287.

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
