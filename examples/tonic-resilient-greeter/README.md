# Tonic Resilient Greeter - gRPC with Resilience Patterns

This example demonstrates real tower-resilience middleware wrapping a gRPC
service built with Tonic, showing both **server-side** and **client-side**
protection.

## Architecture

```
Client                          Server
  ↓                              ↓
CircuitBreakerLayer         RateLimiterLayer (10 req/sec)
  ↓                              ↓
RetryLayer (exponential)    BulkheadLayer (5 concurrent)
  ↓                              ↓
gRPC request    ────────→   ChaosLayer (20% chance of 2s latency)
                                 ↓
                            Greeter logic
```

## Patterns Demonstrated

### Server-Side (Defensive)
- **`RateLimiterLayer`**: caps accepted requests to 10/sec
- **`BulkheadLayer`**: caps in-flight concurrent requests to 5
- **`ChaosLayer`**: injects latency on ~20% of requests -- a test fixture
  that simulates an unreliable backend, not a resilience pattern itself

### Client-Side (Offensive)
- **`CircuitBreakerLayer`**: opens at 50% failure rate (window: 10 calls, min: 3)
- **`RetryLayer`**: exponential backoff (max 3 attempts, starting at 100ms)

## Running the Example

### Start the Server

```bash
# From workspace root
cargo run --bin server

# Or from this directory
cd examples/tonic-resilient-greeter
cargo run --bin server
```

Server listens on `[::1]:50051` (IPv6 localhost).

### Run the Client

In a separate terminal:

```bash
# From workspace root
cargo run --bin client

# Or from this directory
cd examples/tonic-resilient-greeter
cargo run --bin client
```

The client makes 20 requests and demonstrates:
- Successful requests
- Retries on transient failures (the chaos-injected latency causes the
  client's 500ms channel timeout to fire, which `RetryLayer` retries)
- Circuit breaker opening when failures exceed the configured threshold
- Circuit breaker rejecting requests while open

## Implementation Notes

### Why the resilience layers live inside the handler, not around the whole service

Tonic's generated server trait requires `say_hello` to return
`Result<Response<HelloReply>, tonic::Status>`; tower-resilience middleware
produces its own error types (`BulkheadServiceError`, `RateLimiterServiceError`,
`CircuitBreakerError`) when wrapped around a service with `tower::Layer`.
Rather than fighting that mismatch by wiring the layers around the entire
`GreeterServer`/`Channel` (the raw HTTP transport, whose request bodies
aren't `Clone`), this example builds a small, real `tower::Service` per side
-- `Service<HelloRequest>` -- stacks the resilience layers around it with
`ServiceBuilder`, and calls it explicitly from inside the handler, mapping
the resulting error into a `Status`/log line.

This is the same shape used by
[`examples/axum-resilient-kv-store`](../axum-resilient-kv-store): a
resilience-wrapped service held behind an `Arc<Mutex<_>>` (server) or cloned
per call (client), invoked manually, with its error variants matched
explicitly. It is real tower-resilience middleware -- `CircuitBreakerLayer`,
`RetryLayer`, `BulkheadLayer`, `RateLimiterLayer`, `ChaosLayer` are all
imported and doing the actual admission control, backoff, and latency
injection; nothing here is a hand-rolled reimplementation of those patterns.

### Server-side chaos as a test fixture

The `ChaosLayer` is not a resilience pattern -- it exists to make the
server's responses unreliable enough that the client's retry and circuit
breaker layers have something real to react to. It is the innermost layer
in the server pipeline, closest to the greeting logic it perturbs.

## Proto Definition

```protobuf
service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
  rpc SayHelloStream (HelloRequest) returns (stream HelloReply);
}
```

`SayHello` is gated through the full resilience pipeline. `SayHelloStream`
is admission-gated through the same pipeline before the stream is spawned;
the streamed replies themselves are generated independently of the pipeline
call that gated the stream's start.
