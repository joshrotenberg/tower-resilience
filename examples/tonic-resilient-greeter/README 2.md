# Tonic Resilient Greeter - gRPC with Resilience Patterns

This example demonstrates resilience patterns in a gRPC service using Tonic, showing both **server-side** and **client-side** protection.

## Architecture

```
Client                          Server
  ↓                              ↓
Circuit Breaker         Rate Limiter (10 req/sec)
  ↓                              ↓
Retry (exponential)          Bulkhead (5 concurrent)
  ↓                              ↓
gRPC Request    ────────→   Chaos (20% slow)
                                 ↓
                            Greeter Service
```

## Patterns Demonstrated

### Server-Side (Defensive)
- **Bulkhead**: Limits concurrent requests to 5 to protect server resources
- **Rate Limiting**: Conceptually demonstrated (10 req/sec limit)
- **Chaos Engineering**: 20% of requests experience 2-second delays

### Client-Side (Offensive)
- **Circuit Breaker**: Opens at 50% failure rate (window: 10 calls, min: 3)
- **Retry**: Exponential backoff (max 3 attempts, starting at 100ms)

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
- Retries on transient failures
- Circuit breaker opening when failures exceed threshold
- Circuit breaker rejecting requests when open
- Recovery after circuit breaker timeout

## Expected Output

### Server Output
```
Server listening on [::1]:50051
  Bulkhead limit: 5 concurrent requests
  Chaos enabled: 20% slow responses (2s delay)

Received request from: Alice (concurrent: 1/5)
[CHAOS] Injecting slow response for: Alice
Request completed for: Alice (took: 2001ms)
```

### Client Output
```
Starting resilient gRPC client
Making 20 requests to demonstrate resilience patterns...

Request 1: Success - "Hello Alice!" (117ms)
Request 2: [RETRY 1/3] RPC failed, retrying after 100ms...
Request 2: Success after retry - "Hello Alice!" (2234ms)
...
Request 8: Circuit breaker opened! (failure rate: 60%)
Request 9: [CIRCUIT OPEN] Request rejected
...
Request 15: Circuit breaker attempting recovery (half-open)
Request 15: Success - circuit breaker closed
```

## Implementation Notes

### Why Manual Implementation?

The resilience patterns are implemented manually rather than using Tower middleware layers because:

1. **Server Constraints**: Tonic requires services to return `Infallible` errors for proper gRPC error handling. Tower middleware produces typed errors (`BulkheadError`, `RateLimiterError`) that are incompatible with this requirement.

2. **Client Constraints**: Tonic's request bodies are non-Clone (they contain streaming bodies). The retry middleware requires cloneable requests to retry them.

The manual implementations demonstrate the same concepts (state machines, exponential backoff, concurrency limiting) in a Tonic-compatible way.

### Patterns vs Tower Middleware

| Pattern | Tower Middleware | This Example | Reason |
|---------|-----------------|--------------|--------|
| Circuit Breaker | `CircuitBreakerLayer` | Manual state machine | Shows concept without middleware complexity |
| Retry | `RetryLayer` | Manual exponential backoff | Non-Clone request bodies |
| Bulkhead | `BulkheadLayer` | Manual semaphore | Infallible error requirement |
| Rate Limiter | `RateLimiterLayer` | Manual tracking | Infallible error requirement |

## Real-World Usage

For production gRPC services with tower-resilience patterns, consider:

1. **Client-Side**: Use our middleware in a dedicated Tower service layer before calling Tonic
2. **Server-Side**: Implement resilience at the infrastructure level (load balancer, service mesh) or use custom interceptors
3. **Observability**: Add metrics collection to track circuit breaker states, retry counts, and bulkhead utilization

## Testing Different Scenarios

### High Load (Trip Bulkhead)
Modify client to make 10 concurrent requests to see bulkhead rejections.

### Force Circuit Open
Modify chaos rate to 90% to quickly trip the circuit breaker.

### Retry Success
The 20% slow rate naturally demonstrates retry behavior - some requests fail on first attempt but succeed after retry.

## Proto Definition

```protobuf
service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
  rpc SayHelloStream (HelloRequest) returns (stream HelloReply);
}
```

The example focuses on `SayHello` for simplicity. The streaming RPC demonstrates how bulkheads protect against long-running operations.
