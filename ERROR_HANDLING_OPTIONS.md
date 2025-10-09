# Error Handling Options for Issue #74

This document compares different approaches to simplifying error type conversions when composing multiple resilience layers.

## The Problem

When using multiple resilience layers (bulkhead, circuit breaker, rate limiter, etc.), users must write repetitive `From` trait implementations to convert each layer's error type into their application error type.

**Example of current boilerplate:**

```rust
#[derive(Debug)]
enum ServiceError {
    Timeout,
    CircuitOpen,
    BulkheadFull,
    RateLimited,
    AppError(String),
}

// Manual From implementations - THIS IS THE BOILERPLATE
impl From<BulkheadError> for ServiceError {
    fn from(err: BulkheadError) -> Self {
        match err {
            BulkheadError::Timeout => ServiceError::Timeout,
            BulkheadError::BulkheadFull { .. } => ServiceError::BulkheadFull,
        }
    }
}

impl From<CircuitBreakerError> for ServiceError { /* ... */ }
impl From<RateLimiterError> for ServiceError { /* ... */ }
impl From<TimeLimiterError> for ServiceError { /* ... */ }
// ... 4+ implementations for a typical stack
```

## Evaluated Options

### Option 1: Derive Macro (Not Recommended)

**Pros:**
- Cleanest user API
- Declarative syntax

**Cons:**
- Requires proc-macro crate (increased compile time)
- More complex to maintain
- Intrusive (users must derive on their types)
- Overkill for the problem

**Verdict:** ❌ Too complex for the benefit

---

### Option 2: Declarative Macro (Not Recommended)

**Pros:**
- No proc-macro needed
- Somewhat less verbose than manual From

**Cons:**
- Clunky, hard-to-read syntax
- Still feels like boilerplate
- No real advantage over manual From
- Confusing mix of patterns

**Verdict:** ❌ Not worth it - same verbosity, worse DX

---

### Option 3: Documentation Only (Baseline)

**Pros:**
- Zero code changes
- No maintenance burden
- Clear examples

**Cons:**
- Doesn't reduce boilerplate
- Users still write same code

**Verdict:** ⚠️ Acceptable but doesn't solve the problem

---

### Option 4: Helper Trait (Not Recommended)

```rust
pub trait IntoResilienceError<E> {
    fn into_resilience_error(self) -> E;
}

// Still need implementations for each layer
impl IntoResilienceError<MyError> for BulkheadError {
    fn into_resilience_error(self) -> MyError {
        match self {
            BulkheadError::Timeout => MyError::Timeout,
            BulkheadError::BulkheadFull { .. } => MyError::BulkheadFull,
        }
    }
}
// ... 4+ more implementations
```

**Pros:**
- Slightly more semantic naming
- Same control as From trait

**Cons:**
- Same amount of boilerplate as manual From
- No real advantage
- Another trait to import/learn

**Verdict:** ❌ No improvement over manual From

---

### Option 5: Common `ResilienceError<E>` Type ✅ **RECOMMENDED**

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

// Use it everywhere
let service = ServiceBuilder::new()
    .layer(timeout_layer)
    .layer(circuit_breaker)
    .layer(bulkhead)
    .service(my_service);
```

**Pros:**
- ✅ **ZERO boilerplate** - no From implementations
- ✅ Works with any number of layers
- ✅ Rich error context (layer name, counts, durations)
- ✅ Application errors wrapped in `Application` variant
- ✅ Good Display/Debug implementations
- ✅ Helper methods: `is_timeout()`, `is_rate_limited()`, etc.
- ✅ Can still pattern match for custom handling
- ✅ `map_application()` for error transformations

**Cons:**
- ⚠️ Less control over error structure (but you can still wrap it)
- ⚠️ All layers produce same error type (usually what you want)
- ⚠️ May not fit specialized use cases (can still use manual From)

**Verdict:** ✅ **Best for 80-90% of use cases**

## Implementation: ResilienceError

### Type Definition

```rust
pub enum ResilienceError<E> {
    Timeout { layer: &'static str },
    CircuitOpen { name: Option<String> },
    BulkheadFull { concurrent_calls: usize, max_concurrent: usize },
    RateLimited { retry_after: Option<Duration> },
    Application(E),
}
```

### Automatic Conversions

Each resilience crate provides a `From` implementation:

```rust
// In tower-resilience-bulkhead/src/error.rs
impl<E> From<BulkheadError> for ResilienceError<E> {
    fn from(err: BulkheadError) -> Self {
        match err {
            BulkheadError::Timeout => ResilienceError::Timeout { layer: "bulkhead" },
            BulkheadError::BulkheadFull { max_concurrent_calls } => {
                ResilienceError::BulkheadFull {
                    concurrent_calls: max_concurrent_calls,
                    max_concurrent: max_concurrent_calls,
                }
            }
        }
    }
}

// Similar implementations in:
// - tower-resilience-circuitbreaker
// - tower-resilience-ratelimiter  
// - tower-resilience-timelimiter
```

### Helper Methods

```rust
impl<E> ResilienceError<E> {
    pub fn is_timeout(&self) -> bool;
    pub fn is_circuit_open(&self) -> bool;
    pub fn is_bulkhead_full(&self) -> bool;
    pub fn is_rate_limited(&self) -> bool;
    pub fn is_application(&self) -> bool;
    
    pub fn application_error(self) -> Option<E>;
    pub fn map_application<F, T>(self, f: F) -> ResilienceError<T>;
}
```

## Code Size Comparison

| Approach | Boilerplate Lines |
|----------|-------------------|
| Manual From | ~80 lines (4+ implementations) |
| Helper Trait | ~80 lines (same as From) |
| Derive Macro | ~5 lines + proc-macro crate |
| **ResilienceError** | **0 lines** ✨ |

## Usage Examples

### Before (Manual From)

```rust
// Define your error
enum ServiceError {
    Timeout,
    CircuitOpen,
    BulkheadFull,
    RateLimited,
    AppError(MyError),
}

// Write 4+ From implementations (80 lines of boilerplate)
impl From<BulkheadError> for ServiceError { /* ... */ }
impl From<CircuitBreakerError> for ServiceError { /* ... */ }
impl From<RateLimiterError> for ServiceError { /* ... */ }
impl From<TimeLimiterError> for ServiceError { /* ... */ }
```

### After (ResilienceError)

```rust
use tower_resilience_core::ResilienceError;

// That's it!
type ServiceError = ResilienceError<MyAppError>;

// No From implementations needed! ✨
```

## Error Handling Patterns

### Pattern Matching

```rust
match error {
    ResilienceError::Timeout { layer } => {
        log::warn!("Timeout in {}", layer);
    }
    ResilienceError::CircuitOpen { name } => {
        log::error!("Circuit breaker {:?} is open", name);
    }
    ResilienceError::BulkheadFull { concurrent_calls, max_concurrent } => {
        log::warn!("Bulkhead full: {}/{}", concurrent_calls, max_concurrent);
    }
    ResilienceError::RateLimited { retry_after } => {
        log::warn!("Rate limited, retry after {:?}", retry_after);
    }
    ResilienceError::Application(app_err) => {
        log::error!("Application error: {}", app_err);
    }
}
```

### Helper Methods

```rust
if error.is_timeout() {
    // Handle timeout
} else if error.is_application() {
    let app_error = error.application_error().unwrap();
    // Handle application error
}
```

### Error Transformation

```rust
let mapped_error = error.map_application(|app_err| {
    // Transform application error
    app_err.to_string()
});
```

## When NOT to Use ResilienceError

ResilienceError is great for most cases, but you might want manual error handling if:

1. **Very specific error semantics** - You need exact control over error variants
2. **Error recovery logic** - Different layers need different recovery strategies
3. **Legacy code integration** - Existing error types that can't change
4. **Specialized logging** - Custom per-layer error formatting

In these cases, stick with manual `From` implementations or custom error types.

## Recommendation

**Use `ResilienceError<E>` for new projects and typical use cases.**

It provides:
- Zero boilerplate
- Rich error context
- Good debugging experience
- Works with any resilience layer combination
- Easy to upgrade later if needed

**Use manual `From` implementations only when you have specific requirements** that ResilienceError doesn't meet.

## Examples

See these examples for working demonstrations:

- `examples/error_handling_comparison.rs` - Side-by-side comparison
- `examples/resilience_error_demo.rs` - Real-world usage with ResilienceError

## Migration Path

Existing code using manual `From` implementations will continue to work. New code can opt into `ResilienceError<E>` for zero boilerplate.

```rust
// Old code still works
type ServiceError = MyCustomError; // with manual From impls

// New code can use ResilienceError
type ServiceError = ResilienceError<MyAppError>; // zero boilerplate
```
