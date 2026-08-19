# Reconnect factory migration

The reconnect API after 0.11 wraps a service factory rather than an
already-created service. This is a breaking correction: retrying or cloning a
broken service cannot establish a new connection.

Before:

```rust,ignore
let service = ReconnectLayer::new(config).layer(existing_connection);
```

After, for a unit target:

```rust
use std::convert::Infallible;
use tower::{service_fn, Layer};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer};

let make_connection = service_fn(|(): ()| async {
    Ok::<_, Infallible>(service_fn(|request: String| async move {
        Ok::<_, std::io::Error>(request)
    }))
});

let service = ReconnectLayer::new(ReconnectConfig::default()).layer(make_connection);
```

If connection construction needs a target, create the layer with
`ReconnectLayer::for_target(target, config)`. The wrapped factory must implement
`Service<Target>` and return the request-handling service.

Factory errors and connected-service errors are distinct generic parameters on
`ReconnectError`. A classified request failure invalidates the shared
connection generation. Concurrent clones join the same reconnect attempt, and
an automatic request retry is issued only after the replacement service has
been constructed and polled ready.

Set `retry_on_reconnect(false)` for operations whose side effects may have
completed before the connection failure. The failed request is then returned
to the caller, while the replacement connection is prepared for subsequent
requests.
