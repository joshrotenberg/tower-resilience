# Generic fallback-service migration

The existing value, function, predicate, exception, and async-closure fallback
APIs are unchanged. In particular, `FallbackLayer::service(closure)` remains an
async function strategy and does not model Tower readiness.

Use `FallbackLayer::tower_service(backup)` when the backup is an arbitrary
stateful `tower::Service<Request>`:

```rust
use tower::{service_fn, Layer};
use tower_resilience_fallback::FallbackLayer;

#[derive(Debug)]
struct PrimaryError;

let primary = service_fn(|request: String| async move {
    Err::<String, PrimaryError>(PrimaryError)
});
let backup = service_fn(|request: String| async move {
    Ok::<_, std::convert::Infallible>(format!("backup: {request}"))
});

let service = FallbackLayer::<String, String, PrimaryError>::tower_service(backup)
    .name("regional-backup")
    .layer(primary);
```

The returned `ServiceFallbackLayer<B, Request, Response, PrimaryError>` retains
the concrete `B` type. It does not box or erase the backup service.

## Readiness and sharing

Outer `poll_ready` represents primary readiness. Whether the backup is needed
is not known until the primary call completes, so backup readiness is awaited
inside the returned call future. A backup readiness error is therefore returned
from that future as `FallbackError::FallbackFailed`, just like a backup call
error.

The layer and every service cloned or created from it share one backup instance.
The backup need only be `Send`, not `Clone`. Readiness and `call` are driven on
the same locked instance; the lock is released immediately after `call` returns
its future, so backup operations may overlap when the backup service admits
them. Cancelling during readiness releases the lock, and cancelling during the
backup call drops that call future.

## Request ownership

The primary consumes `Request`, while a later backup attempt needs the same
logical request. Generic service fallback therefore requires `Request: Clone`
and clones it once before starting the primary call. Applications with expensive
or non-cloneable requests should place shared payloads behind `Arc`, use a
lightweight replayable request type, or continue using a custom closure strategy
that owns its own reconstruction policy.

## Error selection

`FallbackError` now has a source-compatible defaulted second parameter:

```text
FallbackError<PrimaryError, BackupError = PrimaryError>
```

Existing `FallbackError<E>` uses remain unchanged. For generic backup services:

- a primary readiness error, or a primary call error rejected by `.handle(...)`,
  is `FallbackError::Inner(PrimaryError)`;
- after delegation is selected, a backup readiness or call error is
  `FallbackError::FallbackFailed(BackupError)`;
- the backup error is authoritative and the earlier primary error (or matched
  primary response) is discarded, preserving the existing closure-service
  strategy's last-error-wins behavior.

Use `primary_error()` and `fallback_error()` with different error types. The
existing `inner()`, `into_inner()`, and `map()` methods remain available when
both error types are the same.

## Upstream relationship

This implements the primary-to-backup composition requested by open Tower issue
[`tower-rs/tower#413`](https://github.com/tower-rs/tower/issues/413). Open Tower
issue [`tower-rs/tower#870`](https://github.com/tower-rs/tower/issues/870) is a
different, pre-primary short-circuit/interception primitive; it does not replace
post-failure backup delegation. Both were open when this behavior was added on
2026-08-17.
