# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## CRITICAL: Pre-Push Checklist

**ALWAYS run these commands before EVERY commit and push:**

```bash
# Clean up macOS Finder duplicate files (appear as "file 2.ext", "file 3.ext", etc.)
find . \( -name "* [0-9].*" -o -name "*[0-9].toml" -o -name "*[0-9].md" -o -name "*[0-9].rs" \) \
  -not -path "./.git/*" -not -path "./target/*" -exec rm -f {} \;

# Format, lint, and test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

**All three MUST pass.** If any fail, fix the issues before committing. This prevents CI failures and keeps the codebase clean.

**Note**: macOS Finder sometimes creates duplicate files with numbered suffixes (2, 3, etc.). Always clean these up before committing - they are ignored by .gitignore but may exist in your working directory.

## Project Overview

`tower-resilience` is a comprehensive resilience and fault-tolerance toolkit for Tower services, inspired by Resilience4j. It provides composable middleware patterns for building robust distributed systems in Rust.

**For comprehensive design documentation, feature comparisons, and architectural details, see the rustdoc modules in `crates/tower-resilience/src/` (patterns.rs, composition.rs, use_cases.rs, tower_primer.rs, observability.rs).**

## Workspace Structure

This is a Cargo workspace with multiple crates:

- **tower-resilience-core** - Shared infrastructure (event system, metrics, common utilities)
- **tower-resilience-circuitbreaker** - Circuit breaker pattern implementation
- **tower-resilience-bulkhead** - Bulkhead/resource isolation pattern
- **tower-resilience-timelimiter** - Advanced timeout handling
- **tower-resilience-retry** - Enhanced retry with advanced backoff
- **tower-resilience-cache** - Response memoization
- **tower-resilience-ratelimiter** - Rate limiting for protecting services
- **tower-resilience-chaos** - Chaos engineering for testing (development/testing only)
- **tower-resilience** - Meta-crate re-exporting all modules

## Examples

The `examples/` directory contains practical demonstrations of resilience patterns:

- **axum-resilient-kv-store**: HTTP key-value store with circuit breaker and chaos engineering
  - Run: `cargo run -p axum-resilient-kv-store`
  - Demonstrates: Circuit breaker health checks (`http_status()`, `health_status()`), chaos injection, Kubernetes probes
  
- **tonic-resilient-greeter**: gRPC greeter with server and client resilience
  - Run server: `cargo run --bin server` (from `examples/tonic-resilient-greeter/`)
  - Run client: `cargo run --bin client` (from `examples/tonic-resilient-greeter/`)
  - Demonstrates: Bulkhead (server-side), circuit breaker + retry (client-side), gRPC patterns

**Note on tonic example**: Requires `protoc` (protobuf compiler) to build. Install via:
- Debian/Ubuntu: `apt-get install protobuf-compiler`
- macOS: `brew install protobuf`
- Windows: Download from https://github.com/protocolbuffers/protobuf/releases

## Build and Test Commands

```bash
# Format all code
cargo fmt --all

# Check formatting without modifying files
cargo fmt --all -- --check

# Run clippy with strict checks
cargo clippy --all-targets --all-features -- -D warnings

# Run all tests (lib and integration)
cargo test --workspace --all-features

# Run only library tests
cargo test --lib --all-features --workspace

# Run only integration tests
cargo test --test '*' --all-features --workspace

# Test a specific crate
cargo test -p tower-resilience-circuitbreaker --all-features
cargo test -p tower-resilience-core --all-features

# Test a specific module
cargo test --test circuitbreaker -- concurrency
cargo test --test cache -- eviction_policies

# Build all crates
cargo build --workspace --all-features

# Run benchmarks
cargo bench --bench happy_path_overhead

# Run benchmarks for a specific pattern
cargo bench --bench happy_path_overhead -- circuitbreaker
cargo bench --bench happy_path_overhead -- bulkhead

# Run stress tests (opt-in, marked with #[ignore])
cargo test --test stress -- --ignored --nocapture

# Run stress tests for specific pattern
cargo test --test stress circuitbreaker -- --ignored
cargo test --test stress bulkhead -- --ignored
cargo test --test stress cache -- --ignored
```

## Pre-Push Checklist

**ALWAYS run these commands before pushing or creating a PR:**

```bash
# 1. Format code
cargo fmt --all

# 2. Run clippy
cargo clippy --all-targets --all-features -- -D warnings

# 3. Run all tests
cargo test --workspace --all-features
```

All three must pass before pushing. No exceptions.

## Documentation Policy

**NEVER commit bespoke markdown design documents** to the repository (e.g., `ERROR_HANDLING_OPTIONS.md`, `DESIGN_IDEAS.md`, etc.). 

- ✅ Use `CLAUDE.md` for project-specific guidance to Claude Code
- ✅ Use rustdoc modules in `crates/tower-resilience/src/*.rs` for comprehensive architecture/design documentation
- ✅ Use inline doc comments (`///`) for API documentation
- ✅ Use `README.md` files for user-facing documentation
- ❌ Do NOT create standalone design exploration markdown files
- ❌ Do NOT commit temporary analysis or brainstorming documents

If you need to explore design ideas, do it in conversation or update existing rustdoc modules in `crates/tower-resilience/src/`.

## Example Naming Convention

**CRITICAL FOR WINDOWS CI**: Never name examples `simple.rs` or use duplicate filenames across crates.

On Windows, all example binaries with the same name (e.g., `simple.exe`) will collide during parallel builds, causing CI failures.

**Naming pattern**: Use descriptive, unique names per crate:
- ✅ `crates/tower-circuitbreaker/examples/circuitbreaker_example.rs`
- ✅ `crates/tower-cache/examples/cache_example.rs`
- ✅ `crates/tower-bulkhead/examples/bulkhead_demo.rs`
- ❌ `crates/tower-circuitbreaker/examples/simple.rs` (NEVER USE)
- ❌ `crates/tower-cache/examples/simple.rs` (NEVER USE)

When adding new examples, use pattern: `{crate-feature}_example.rs` or `{crate-feature}_demo.rs`

## Architecture Overview

### Core Infrastructure (tower-resilience-core)

**Event System** (`src/events.rs`):
- `ResilienceEvent` trait - Base trait for all events (provides event_type, timestamp, pattern_name)
- `EventListener<E>` trait - Callback interface for events
- `EventListeners<E>` - Collection managing multiple listeners with `emit()` method
- `FnListener<E, F>` - Function-based listener implementation for convenience

Events are the backbone of observability. All resilience patterns emit events at key decision points, allowing users to hook in custom logic for logging, metrics, alerting, etc.

**Design Philosophy**:
- Events over direct metrics/tracing calls
- Composable and testable
- Zero-cost when no listeners registered

### Circuit Breaker (tower-resilience-circuitbreaker)

Migrated from standalone crate with event system integration:

**Core Components**:
- `Circuit` (src/circuit.rs) - State machine managing Closed/Open/HalfOpen transitions
- `CircuitBreaker<S>` (src/lib.rs) - Tower Service wrapper applying circuit breaker logic
- `CircuitBreakerConfig` (src/config.rs) - Configuration parameters with event listener support
- `CircuitBreakerLayer` (src/layer.rs) - Tower Layer (manual .layer() method, not Tower trait)
- `CircuitBreakerEvent` (src/events.rs) - Event enum for observability

**Event System Integration**:
- Events: StateTransition, CallPermitted, CallRejected, SuccessRecorded, FailureRecorded, SlowCallDetected
- Builder methods: `on_state_transition()`, `on_call_permitted()`, `on_call_rejected()`, `on_success()`, `on_failure()`, `on_slow_call()`
- All state changes and call decisions emit events
- Metrics/tracing still supported via feature flags, working alongside events

**Key Features**:
- `Arc<Mutex<Circuit>>` for shared state across service clones (required by Tower)
- Count-based sliding window for failure rate calculation
- **Slow call detection**: Configurable duration threshold and slow call rate threshold
  - Circuit opens if either failure rate OR slow call rate exceeds threshold
  - Automatic call duration measurement
  - Slow call events emitted for observability
- State transitions reset counters (success_count, failure_count, total_count, slow_call_count)
- Custom failure classifiers via Arc<dyn Fn>
- Fallback handler support for graceful degradation
- Named instances for observability (defaults to `<unnamed>`)

**Remaining Enhancements** (tracked in GitHub issues):
- Time-based sliding window (#6)
- Sync state inspection (#11)

### Design Patterns Used Throughout

**Builder Pattern**: All patterns use fluent builders
```rust
CircuitBreakerLayer::builder()
    .failure_rate_threshold(0.5)
    .sliding_window_size(100)
    .on_state_transition(|from, to| { /* ... */ })
    .build()
```

**Event-Driven Observability**: Patterns emit events, don't call metrics directly
```rust
.on_call_permitted(|| { /* custom logic */ })
.on_call_rejected(|| { /* custom logic */ })
```

**Tower Layer Composition**: Stack patterns together
```rust
ServiceBuilder::new()
    .layer(timeout_layer)
    .layer(circuit_breaker_layer)
    .layer(bulkhead_layer)
    .layer(retry_layer)
    .service(my_service)
```

## Development Guidelines

### Adding New Resilience Patterns

1. Create crate: `crates/tower-{pattern}/`
2. Add to workspace members in root `Cargo.toml`
3. Depend on `tower-resilience-core` for event infrastructure
4. Define pattern-specific events implementing `ResilienceEvent`
5. Implement Tower `Layer` and `Service` traits
6. Provide builder with fluent API and event listener hooks
7. Write comprehensive tests (unit + integration)
8. Add examples showing usage
9. Update workspace README and this CLAUDE.md

### Event System Usage Pattern

When implementing a new resilience pattern:

1. **Define Events Enum**:
```rust
#[derive(Debug, Clone)]
pub enum BulkheadEvent {
    CallPermitted { pattern_name: String, timestamp: Instant },
    CallRejected { pattern_name: String, timestamp: Instant },
}
```

2. **Implement ResilienceEvent**:
```rust
impl ResilienceEvent for BulkheadEvent {
    fn event_type(&self) -> &'static str { /* ... */ }
    fn timestamp(&self) -> Instant { /* ... */ }
    fn pattern_name(&self) -> &str { /* ... */ }
}
```

3. **Store EventListeners in Config**:
```rust
pub struct BulkheadConfig {
    // ... other config
    event_listeners: EventListeners<BulkheadEvent>,
}
```

4. **Emit Events at Decision Points**:
```rust
let event = BulkheadEvent::CallPermitted { /* ... */ };
config.event_listeners.emit(&event);
```

5. **Provide Builder Methods**:
```rust
pub fn on_call_permitted<F>(mut self, f: F) -> Self
where F: Fn() + Send + Sync + 'static
{
    self.event_listeners.add(FnListener::new(move |event| {
        if matches!(event, BulkheadEvent::CallPermitted { .. }) {
            f();
        }
    }));
    self
}
```

### Testing Strategy

#### Test Organization

Tests are organized into pattern-specific modules in `tests/`:

```
tests/
  core.rs                  # Top-level core tests
  core/
    mod.rs                 # Core event system tests
    events.rs              # Event emission and listeners
    concurrency.rs         # Concurrent event handling
    fn_listener.rs         # Function listener tests
    lifecycle.rs           # Event lifecycle tests
    panics.rs              # Panic handling in listeners
  circuitbreaker.rs        # Top-level circuit breaker tests
  circuitbreaker/
    mod.rs                 # Module declaration
    integration.rs         # Basic integration tests
    concurrency.rs         # P0 - Concurrent access patterns
    config_validation.rs   # P0 - Configuration edge cases
    thresholds.rs          # P0 - Threshold precision
    time_based.rs          # P0 - Time-based window behavior
    combinations.rs        # P1 - Feature combinations
    half_open.rs           # P1 - Half-open state complexity
    reset.rs               # P1 - Reset functionality
    edge_cases.rs          # P2 - Event listeners, failure classifiers
  bulkhead/
    mod.rs                 # Module declaration
    integration.rs         # Basic integration tests
    concurrency.rs         # Concurrent request handling
    timeout.rs             # Timeout behavior
    edge_cases.rs          # Edge case handling
  cache/
    mod.rs                 # Module declaration
    cache_layer.rs         # Layer integration tests
    cache_concurrency.rs   # Concurrent cache access
    cache_key_extraction.rs # Key extraction logic
    eviction_policies.rs   # LRU, LFU, FIFO eviction
    cache_edge_cases.rs    # Edge cases
  retry/
    mod.rs                 # Module declaration
    retry_behavior.rs      # Retry logic tests
    retry_backoff.rs       # Backoff strategy tests
    retry_config.rs        # Configuration validation
    retry_predicates.rs    # Retry predicate tests
  stress.rs                # Stress tests (opt-in with #[ignore])
```

**Priority Levels**:
- **P0** (Critical): Core functionality, concurrency, edge cases that must work
- **P1** (High): Feature combinations, complex scenarios
- **P2** (Medium): Additional edge cases, performance tests

**Test Types**:
- **Unit tests**: In each crate's `src/` directories, testing individual components
- **Integration tests**: In `tests/` directory, testing full Tower Service integration
- **Stress tests**: In `tests/stress.rs`, marked with `#[ignore]` for opt-in execution
- **Benchmarks**: In `benches/` directory, measuring performance overhead

#### Test Patterns

- **Unit tests** in each crate test core logic in isolation
- **Integration tests** test full Tower Service integration
- Use `tokio::test` for async tests
- Test event listeners with `Arc<AtomicUsize>` counters
- Use `tower::service_fn` for creating test services
- Test error paths and edge cases
- For circuit breakers/bulkheads: test state transitions thoroughly

#### Timing Tests and Windows Compatibility

**IMPORTANT**: Windows has significantly less precise timers than Linux/macOS. When writing timing-sensitive tests:

1. **Use generous tolerances for timing assertions**:
   - ✅ Good: `±30ms` tolerance for sub-100ms timeouts
   - ❌ Bad: `±10ms` tolerance (will fail on Windows)

2. **Examples from actual tests**:
   ```rust
   // Timeout precision test - use 30ms tolerance for Windows
   const TOLERANCE_MS: u64 = 30;  // Not 10!
   
   // Test that validates timeout occurred before service completion
   assert!(elapsed.as_millis() < 60, "Expected ~30ms, got {}ms", elapsed.as_millis());
   // Not: assert!(elapsed.as_millis() < 45);  // Too tight for Windows
   ```

3. **General guidelines**:
   - For timeouts <100ms: use ±30ms tolerance minimum
   - For timeouts 100-500ms: use ±50ms tolerance
   - For timeouts >500ms: use ±100ms tolerance or 10-20% of timeout duration
   - Always include descriptive assertion messages showing actual vs expected

4. **Why Windows is different**:
   - Windows timer resolution is typically 15.6ms (vs ~1ms on Linux)
   - Task scheduling is less predictable
   - System load affects timing more significantly
   - CI runners may have additional variance

5. **CI failures**: If a timing test passes locally (macOS/Linux) but fails on Windows CI:
   - Increase the tolerance margins
   - Verify the test is validating behavior, not exact timing
   - Consider if the test is too brittle for cross-platform compatibility

**Example Test Pattern**:
```rust
#[tokio::test]
async fn event_listeners_are_called() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);
    
    let layer = PatternLayer::builder()
        .on_event(move || { c.fetch_add(1, Ordering::SeqCst); })
        .build();
    
    // trigger event
    
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
```

#### Stress Tests

Stress tests validate behavior under extreme conditions and are located in `tests/stress/`:

```
tests/stress/
  mod.rs              # Shared utilities (ConcurrencyTracker, memory tracking)
  bulkhead.rs         # Bulkhead stress tests (7 tests)
  cache.rs            # Cache stress tests (8 tests)
  circuitbreaker.rs   # Circuit breaker stress tests (7 tests)
  composition.rs      # Layer composition stress tests (6 tests)
  
  # TODO: Add stress tests for retry, ratelimiter, timelimiter, chaos patterns
```

**What Stress Tests Validate**:
- High volume (100k-1M operations)
- High concurrency (1000+ concurrent requests)
- Memory usage and leak detection
- State consistency under stress
- Resource cleanup (no panics, deadlocks, leaks)
- Performance degradation under load

**Running Stress Tests**:
```bash
# Run all stress tests
cargo test --test stress -- --ignored --nocapture

# Run specific pattern
cargo test --test stress circuitbreaker -- --ignored --nocapture

# Run specific test
cargo test --test stress circuitbreaker::stress_one_million_calls -- --ignored --nocapture
```

**Automated Stress Testing**:
- Stress tests run nightly via GitHub Actions (`.github/workflows/stress-tests.yml`)
- Tests run at 02:00 UTC every night
- Each pattern runs independently with 60-minute timeout
- Failures automatically create GitHub issues with:
  - Pattern name
  - Run URL
  - Date/time
  - Links to logs
- Issues tagged with `stress-test-failure`, `pattern:<name>`, `needs-investigation`
- Can be manually triggered via workflow_dispatch

**When to Add Stress Tests**:
- New resilience patterns
- Changes to concurrency handling
- Memory management changes
- Performance-critical code paths

**Stress Test Utilities** (in `tests/stress/mod.rs`):
```rust
// Track peak concurrency
let tracker = ConcurrencyTracker::new();
tracker.enter();  // In task
tracker.exit();   // On completion
let peak = tracker.peak();

// Memory usage (macOS only, returns 0.0 on other platforms)
let mem_start = get_memory_usage_mb();
// ... operations ...
let mem_end = get_memory_usage_mb();
let mem_delta = mem_end - mem_start;
```

## Key Implementation Notes

- **Rust Edition**: Published crates use 2021 edition (MSRV 1.64.0, matching Tower's MSRV policy)
  - Root workspace Cargo.toml uses edition "2024" for development
  - All published crates in `crates/` use edition "2021" for compatibility
  - When adding new crates, ensure `edition = "2021"` in their Cargo.toml
- **Documentation**: All public APIs must have doc comments
- **Error Handling**: Use `thiserror` for library errors (not `anyhow`)
- **Optional Features**: `metrics`, `tracing` (use `#[cfg(feature = "...")]`)
- **Async Runtime**: Tokio only
- **Tower Service Cloning**: Use `Arc` for shared state that needs to survive clones
- **Builder Pattern**: All builder methods return `Self` for method chaining
- **Naming**: Pattern names are optional but recommended for observability

## Layer Composition and ServiceBuilder

### Known Limitations

**CRITICAL**: Composing 3+ resilience layers together using `ServiceBuilder` often hits Rust trait bound limitations. This is a known issue with complex Tower layer stacks.

#### What Works Well (2 layers)
```rust
// ✅ Cache + Retry
ServiceBuilder::new()
    .layer(CacheConfig::builder().build().layer())
    .layer(RetryConfig::builder().build().layer())
    .service_fn(|req| async { /* ... */ })

// ✅ Bulkhead + Retry
ServiceBuilder::new()
    .layer(BulkheadConfig::builder().build())
    .layer(RetryConfig::builder().build().layer())
    .service_fn(|req| async { /* ... */ })

// ✅ TimeLimiter + CircuitBreaker
ServiceBuilder::new()
    .layer(TimeLimiterConfig::builder().build().layer())
    .layer(CircuitBreakerConfig::builder().build())
    .service_fn(|req| async { /* ... */ })
```

#### What Often Fails (3+ layers)
```rust
// ❌ Complex stacks hit trait bound errors
ServiceBuilder::new()
    .layer(CacheConfig::builder().build().layer())
    .layer(TimeLimiterConfig::builder().build().layer())
    .layer(CircuitBreakerConfig::builder().build())
    .layer(RetryConfig::builder().build().layer())
    .service_fn(|req| async { /* ... */ })
// Error: CircuitBreakerLayer<Res, Err>: Layer<Retry<_, E>>` not satisfied
```

### Workarounds for Complex Stacks

1. **Apply Layers at Different Architectural Points**
   ```rust
   // At HTTP client layer: Cache + Timeout
   let http_client = ServiceBuilder::new()
       .layer(cache_layer)
       .layer(timeout_layer)
       .service(base_http);
   
   // At business logic layer: Circuit Breaker + Retry
   let resilient_client = ServiceBuilder::new()
       .layer(circuit_breaker)
       .layer(retry_layer)
       .service(http_client);
   ```

2. **Manual Layer Composition**
   ```rust
   // Build layers inside-out manually
   let with_retry = retry_layer.layer().layer(base_service);
   let with_circuit_breaker = circuit_breaker_layer.layer(with_retry);
   let service = cache_layer.layer().layer(with_circuit_breaker);
   ```

3. **Limit Stack Depth**
   - Keep ServiceBuilder stacks to 2-3 layers max
   - Use manual composition for deeper stacks
   - Apply patterns at different system layers (network, application, business logic)

### Layer API Inconsistencies

**IMPORTANT**: Not all layers have consistent APIs:

| Layer | Builder Returns | Usage in ServiceBuilder |
|-------|----------------|-------------------------|
| Cache | Config | `.layer(config.build().layer())` |
| Retry | Config | `.layer(config.build().layer())` |
| TimeLimiter | Config | `.layer(config.build().layer())` |
| RateLimiter | Config | `.layer(config.build().layer())` |
| Bulkhead | Layer | `.layer(config.build())` |
| CircuitBreaker | Layer | `.layer(config.build())` |

**Why the difference**: Bulkhead and CircuitBreaker return `Layer` directly from `.build()` for ergonomics, while others return `Config` with a `.layer()` method. This is a known inconsistency being tracked for standardization.

### Error Type Integration

**Recommended Approach: Use `ResilienceError<E>`**

The simplest way to handle errors from multiple resilience layers is to use `ResilienceError<E>`:

```rust
use tower_resilience_core::ResilienceError;

// Your application error
#[derive(Debug)]
enum AppError {
    DatabaseDown,
    InvalidRequest,
}

// That's it! No From implementations needed
type ServiceError = ResilienceError<AppError>;

// All resilience layer errors automatically convert
let service = ServiceBuilder::new()
    .layer(timeout_layer)
    .layer(circuit_breaker)
    .layer(bulkhead)
    .service(my_service);

// Check error types with convenience methods
match result {
    Err(e) if e.is_timeout() => { /* handle timeout */ },
    Err(e) if e.is_rate_limited() => { /* handle rate limit */ },
    Err(e) if e.is_circuit_open() => { /* handle circuit open */ },
    _ => {}
}
```

**Benefits**:
- Zero boilerplate - no `From` trait implementations required
- Rich error context (layer names, counts, durations)
- Convenient helper methods: `is_timeout()`, `is_rate_limited()`, `is_circuit_open()`, etc.
- Works seamlessly with all resilience layers

**Alternative: Manual Error Conversions**

For specific use cases where you need custom error types, implement `From` conversions:

```rust
// Your application error type
#[derive(Debug, Clone)]
enum MyError {
    Network(String),
    Timeout,
    CircuitOpen,
    BulkheadFull,
}

// Manual implementations for each resilience error type
impl From<tower_resilience_bulkhead::BulkheadError> for MyError {
    fn from(err: tower_resilience_bulkhead::BulkheadError) -> Self {
        match err {
            tower_resilience_bulkhead::BulkheadError::Timeout => MyError::Timeout,
            tower_resilience_bulkhead::BulkheadError::BulkheadFull { .. } => MyError::BulkheadFull,
        }
    }
}

impl<E> From<tower_resilience_circuitbreaker::CircuitBreakerError<E>> for MyError {
    fn from(_: tower_resilience_circuitbreaker::CircuitBreakerError<E>) -> Self {
        MyError::CircuitOpen
    }
}

impl<E> From<tower_resilience_timelimiter::TimeLimiterError<E>> for MyError {
    fn from(_: tower_resilience_timelimiter::TimeLimiterError<E>) -> Self {
        MyError::Timeout
    }
}

impl From<tower_resilience_ratelimiter::RateLimiterError> for MyError {
    fn from(_: tower_resilience_ratelimiter::RateLimiterError) -> Self {
        MyError::Timeout
    }
}
```

**Note**: Generic error parameters (`CircuitBreakerError<E>`, `TimeLimiterError<E>`) require generic `From` impls because these errors can wrap inner service errors.

### Event Listener Signatures

Event listener callbacks have varying signatures depending on the pattern:

```rust
// No parameters
CacheConfig::builder()
    .on_hit(|| { println!("Cache hit"); })
    .on_miss(|| { println!("Cache miss"); })

// Duration parameter
RateLimiterConfig::builder()
    .on_permit_acquired(|wait_duration: Duration| {
        println!("Waited {:?}", wait_duration);
    })

// usize parameter
BulkheadConfig::builder()
    .on_call_permitted(|concurrent: usize| {
        println!("Concurrent calls: {}", concurrent);
    })

// State transition parameters
CircuitBreakerConfig::builder()
    .on_state_transition(|from: CircuitState, to: CircuitState| {
        println!("{:?} -> {:?}", from, to);
    })
```

**Best Practice**: Check the builder method docs or tests for exact signatures. The type system will catch mistakes, but it's faster to reference examples.

## Stress Tests

Stress tests validate pattern behavior under extreme conditions and are opt-in (marked with `#[ignore]`).

**Running Stress Tests**:
```bash
# Run all stress tests
cargo test --test stress -- --ignored --nocapture

# Run stress tests for specific pattern
cargo test --test stress circuitbreaker -- --ignored
cargo test --test stress bulkhead -- --ignored
cargo test --test stress cache -- --ignored
```

**What Stress Tests Validate**:
- **High Volume**: Millions of operations to detect race conditions and performance degradation
- **High Concurrency**: Thousands of concurrent requests to validate thread safety
- **Memory Stability**: Leak detection and bounded growth under sustained load
- **State Consistency**: Correctness of state management under extreme conditions
- **Pattern Composition**: Layered middleware behavior at scale

**Example Results** (from actual runs):
- Circuit Breaker: 1M calls in ~2.8s (357k calls/sec)
- Bulkhead: 10k fast operations in ~56ms (176k ops/sec)  
- Cache: 100k entries fill + hit test validates performance

**When to Run**:
- Before major releases
- After significant refactoring
- When investigating performance issues
- When adding new concurrency-sensitive features

**Adding New Stress Tests**:
1. Add test function in `tests/stress.rs`
2. Mark with `#[ignore]` attribute
3. Use high operation counts (100k-1M+)
4. Include timing measurements
5. Validate correctness, not just performance

## Benchmarking

Benchmarks measure the overhead of resilience patterns in the happy path (no failures, permits available, etc.).

**Running Benchmarks**:
```bash
# Run all benchmarks
cargo bench --bench happy_path_overhead

# Run specific pattern benchmark
cargo bench --bench happy_path_overhead -- circuitbreaker
cargo bench --bench happy_path_overhead -- bulkhead
cargo bench --bench happy_path_overhead -- cache
```

**Benchmark Structure**:
- Located in `benches/happy_path_overhead.rs`
- Uses Criterion for statistical analysis
- Measures latency per operation
- Compares against baseline (no middleware)

**Expected Overhead** (approximate):
- Baseline: ~10 ns
- Retry: ~80-100 ns
- Time Limiter: ~107 ns
- Rate Limiter: ~124 ns
- Bulkhead: ~162 ns
- Cache (hit): ~250 ns
- Circuit Breaker: ~298 ns
- Circuit Breaker + Bulkhead: ~413 ns

**When to Run**:
- After performance optimizations
- When adding new features to existing patterns
- To validate overhead remains acceptable
- Before major releases

## Dependency Updates

When updating major dependencies, be aware of API changes:

**Recent Updates** (as of latest):
- **axum 0.7 → 0.8**: Direct version bump, no API changes needed
- **criterion 0.5 → 0.7**: `criterion::black_box` moved to `std::hint::black_box`
- **rand 0.8 → 0.9**: Several API renames:
  - `gen()` → `random()`
  - `gen_range()` → `random_range()`
  - `thread_rng()` → `rng()`
  - `from_entropy()` → `from_os_rng()`
- **tonic 0.12 → 0.14**: Major build system changes:
  - `tonic-build` → `tonic-prost-build` (build dependency)
  - Added `tonic-prost` runtime dependency
  - `tonic_build::compile_protos()` → `tonic_prost_build::compile_protos()`

## Common Pitfalls

1. **Service Cloning**: Tower services get cloned - use `Arc` for shared state
2. **Async Lifetimes**: Be careful with lifetimes in async functions
3. **Event Emission**: Don't forget to emit events at all decision points
4. **Feature Flags**: Test with `--all-features` to catch feature-gated code issues
5. **Poll Ready**: Don't forget to implement `poll_ready` in Service implementations
6. **Complex Layer Stacks**: Limit `ServiceBuilder` to 2-3 layers; use manual composition or architectural separation for deeper stacks
7. **Error Type Conversions**: Always implement `From` conversions for all resilience error types used in your stack (or use `ResilienceError<E>` wrapper)
8. **Event Listener Signatures**: Verify callback signatures from docs/tests - they vary by pattern
9. **Rust Edition**: Always use edition "2021" for published crates in `crates/`, not "2024"
10. **CI Requirements**: The tonic example requires `protoc` to be installed in CI - ensure it's in the workflow

## Feature Comparison vs Tower Built-ins

Tower already has some resilience patterns, but tower-resilience offers enhanced versions and additional patterns:

**What Tower Has**:
- Timeout (basic)
- Retry (basic exponential backoff)
- Rate limiting
- Concurrency limiting
- Load shedding

**What We Add**:
- Circuit breaker (doesn't exist in Tower)
- True bulkhead pattern with resource isolation
- Advanced timeout (with cancellation control)
- Enhanced retry (IntervalFunction abstraction, better backoff control)
- Response caching
- Unified event/metrics system across all patterns
- Consistent builder APIs

For detailed comparisons and design rationale, see the rustdoc modules in `crates/tower-resilience/src/patterns.rs`.

## References

- **Tower**: https://docs.rs/tower
- **Resilience4j** (inspiration): https://resilience4j.readme.io/
- **Documentation**: See rustdoc modules in `crates/tower-resilience/src/` for comprehensive guides
- **Original Circuit Breaker**: `../tower-circuitbreaker` (standalone crate)
