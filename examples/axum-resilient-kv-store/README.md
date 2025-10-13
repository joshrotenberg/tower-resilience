# Axum Resilient Key-Value Store Example

This example demonstrates the new `http_status()` and `health_status()` helper methods from PR #121 for implementing health check endpoints with circuit breakers.

## Running

From the workspace root:

```bash
cargo run -p axum-resilient-kv-store
```

Or from this directory:

```bash
cd examples/axum-resilient-kv-store
cargo run
```

## Testing

```bash
# Store a value
curl -X POST http://localhost:3000/mykey -d "Hello, World!"

# Retrieve a value
curl http://localhost:3000/mykey

# Check readiness (returns 200 when circuit closed, 503 when open)
curl -i http://localhost:3000/health/ready

# Check liveness (always returns 200)
curl http://localhost:3000/health/live

# Manually open the circuit (for testing)
curl -X POST http://localhost:3000/admin/circuit/open

# Check readiness again (should now return 503)
curl -i http://localhost:3000/health/ready

# Close the circuit
curl -X POST http://localhost:3000/admin/circuit/close
```

## Features Demonstrated

- **Circuit Breaker Health Integration**: Shows how to use `http_status()` and `health_status()` helper methods
- **Kubernetes-Ready Probes**: `/health/ready` and `/health/live` endpoints
- **Manual Circuit Control**: Admin endpoints for testing circuit behavior
- **Simple KV Store**: Basic GET/POST operations on an in-memory store
