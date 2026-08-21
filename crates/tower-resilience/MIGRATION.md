# Migrating from 0.12 to 0.13

Version 0.13 is a pre-1.0 minor release with intentional source-breaking
changes. Release tooling may describe a `0.12 -> 0.13` bump as API compatible
because that version increment is allowed to contain breaking changes; it does
not mean existing 0.12 source code will compile unchanged.

The central migration rule is: configuration that could previously create an
invalid runtime value is now validated at construction time. Handle the new
typed `Result` where the layer, algorithm, budget, or backoff is built.

## Fallible construction

The following APIs now return `Result`:

| Crate | APIs | Error type |
| --- | --- | --- |
| `core` | `AimdController::new` | `AimdConfigError` |
| `adaptive` | `Aimd::new`, `AimdBuilder::build` | `AimdConfigError` |
| `adaptive` | `VegasBuilder::build` | `VegasConfigError` |
| `bulkhead` | `BulkheadConfigBuilder::{build, build_with_handle}` | `BulkheadConfigError` |
| `circuitbreaker` | `CircuitBreakerConfigBuilder::{build, build_with_handle}` | `CircuitBreakerConfigError` |
| `ratelimiter` | `RateLimiterConfigBuilder::{build, build_with_handle}` | `RateLimiterConfigError` |
| `outlier` | `OutlierDetectionConfigBuilder::build` | `OutlierDetectionConfigError` |
| `retry` | `ExponentialBackoff::multiplier`, `ExponentialRandomBackoff::{new, multiplier}` | `BackoffConfigError` |
| `retry` | `TokenBucketBuilder::build`, `AimdBudgetBuilder::build`, `TokenBucketBudget::new`, `AimdBudget::new` | `RetryBudgetConfigError` |
| `reconnect` | `ReconnectPolicy::exponential_random` | `BackoffConfigError` |

### Propagate configuration errors

For configuration loaded at runtime, prefer returning the typed error or using
`?` to compose it into an application configuration error:

```rust
# #[cfg(all(feature = "bulkhead", feature = "circuitbreaker", feature = "ratelimiter"))]
# {
use std::error::Error;
use tower_resilience::bulkhead::BulkheadLayer;
use tower_resilience::circuitbreaker::CircuitBreakerLayer;
use tower_resilience::core::aimd::{AimdConfig, AimdController};
use tower_resilience::ratelimiter::RateLimiterLayer;

fn build_layers() -> Result<(), Box<dyn Error>> {
    let _controller = AimdController::new(AimdConfig::default())?;
    let _bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(32)
        .build()?;

    let _circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .build()?;

    let _rate_limiter = RateLimiterLayer::per_second(100).build()?;
    Ok(())
}
# build_layers().unwrap();
# }
```

For a hard-coded preset or static configuration, `expect` is reasonable when
the message explains the invariant:

```rust
# #[cfg(feature = "adaptive")]
# {
use tower_resilience::adaptive::Aimd;

let algorithm = Aimd::builder()
    .build()
    .expect("the default AIMD configuration is valid");
# let _ = algorithm;
# }
```

### Shared handles

In 0.12, callers could access the layer directly from the returned tuple:

```rust,ignore
let circuit_layer = CircuitBreakerLayer::builder().build_with_handle().0;
```

In 0.13, tuple access must happen after handling the result:

```rust
# #[cfg(all(feature = "bulkhead", feature = "circuitbreaker"))]
# {
use std::error::Error;
use tower_resilience::bulkhead::BulkheadLayer;
use tower_resilience::circuitbreaker::CircuitBreakerLayer;

fn shared_layers() -> Result<(), Box<dyn Error>> {
    let (_circuit_layer, circuit_handle) = CircuitBreakerLayer::builder()
        .build_with_handle()?;
    let (_bulkhead_layer, bulkhead_handle) = BulkheadLayer::builder()
        .max_concurrent_calls(100)
        .build_with_handle()?;

    // Retain the handles when state must be shared or inspected externally.
    let _ = (circuit_handle, bulkhead_handle);
    Ok(())
}
# shared_layers().unwrap();
# }
```

This is the migration shape for integrations such as GovCraft/Acton that
previously called `build_with_handle().0`: change the surrounding helper to
return `Result<Option<Layer>, ConfigError>` (or an application error), then use
`build_with_handle()?.0`.

Integrations such as RMQTT that previously returned `builder.build()` directly
must likewise update their helper's return type:

```rust,ignore
// 0.12
fn circuit_breaker_layer() -> CircuitBreakerLayer {
    CircuitBreakerLayer::builder().build()
}
```

```rust
# #[cfg(feature = "circuitbreaker")]
# {
use tower_resilience::circuitbreaker::{
    CircuitBreakerConfigError, CircuitBreakerLayer,
};

// 0.13
fn circuit_breaker_layer(
) -> Result<CircuitBreakerLayer, CircuitBreakerConfigError> {
    CircuitBreakerLayer::builder().build()
}
# circuit_breaker_layer().unwrap();
# }
```

Alternatively, use `expect` only when an earlier validation step makes invalid
configuration impossible.

### Backoff and retry budgets

Backoff modifiers now validate floating-point inputs immediately:

```rust
# #[cfg(all(feature = "retry", feature = "reconnect"))]
# {
use std::error::Error;
use std::time::Duration;
use tower_resilience::reconnect::ReconnectPolicy;
use tower_resilience::retry::{ExponentialBackoff, RetryBudgetBuilder};

fn retry_configuration() -> Result<(), Box<dyn Error>> {
    let _backoff = ExponentialBackoff::new(Duration::from_millis(100))
        .multiplier(2.0)?;
    let _reconnect = ReconnectPolicy::exponential_random(
        Duration::from_millis(100),
        Duration::from_secs(10),
        0.25,
    )?;
    let _budget = RetryBudgetBuilder::new()
        .token_bucket()
        .tokens_per_second(10.0)
        .max_tokens(100)
        .build()?;
    Ok(())
}
# retry_configuration().unwrap();
# }
```

## Outlier future type

`OutlierDetectionService::Future` is now the named
`OutlierDetectionFuture<F, C>` instead of a boxed future. Normal `.await`
callers do not need to change. Code that names the associated future type or
requires a boxed future must update the type or box it at that boundary. The
new future removes one heap allocation per call and removes the previous
`Send + 'static` bounds from the service implementation.

`OutlierDetectionConfigBuilder::build` also returns
`Result<_, OutlierDetectionConfigError>` and rejects a missing detector during
construction.

## Bulkhead errors

`BulkheadError` adds `Closed` and `NotReady`. Code that exhaustively matches
the enum must handle both variants. They distinguish a permanently closed
semaphore from calling a backpressure service without first obtaining a
readiness reservation.

## Generic error wrappers and `Error::source`

Generic middleware errors now implement `std::error::Error` for any inner type
that implements `Debug + Display`. This allows Tower's
`Box<dyn Error + Send + Sync>` (`BoxError`) to compose directly through the
middleware stack.

As part of that change, generic application/inner errors are no longer returned
from `std::error::Error::source`. Typed access is unchanged: match the wrapper
variant or use its `inner`, `into_inner`, `application_error`, `primary_error`,
or `fallback_error` accessor as appropriate. Code that walked `source()` to
recover an application error should migrate to those typed APIs.

The new generic Tower backup-service fallback is additive, and the second error
parameter on `FallbackError<PrimaryError, BackupError = PrimaryError>` is
defaulted. See the fallback crate's migration module for its readiness,
request-cloning, and error-selection behavior.

## Time limiter preset rename

`TimeLimiterLayer::streaming()` is deprecated. Use
`TimeLimiterLayer::detached()` for the same preset. The new name makes the
actual boundary explicit: the timeout covers the `Service::call` future, not
readiness or later response-body frames.

## Retry-budget unwind safety

`TokenBucketBudget` no longer automatically implements `UnwindSafe` or
`RefUnwindSafe` after its accounting moved to a clock-backed synchronized
state. Most async applications are unaffected. Code with explicit unwind-safety
bounds must remove that bound, introduce an application-owned wrapper, or use
`AssertUnwindSafe` only after reviewing its own panic invariants.
