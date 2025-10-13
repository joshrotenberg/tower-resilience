# Axum Resilient Key-Value Store with Chaos Engineering

This example demonstrates:
1. **Circuit Breaker + Health Check Integration**: Using the new `http_status()` and `health_status()` helper methods from PR #121
2. **Chaos Engineering**: Configurable failure injection to test resilience
3. **Kubernetes-Ready Probes**: `/health/ready` and `/health/live` endpoints

The circuit breaker automatically responds to chaos-injected failures, demonstrating real resilience in action.

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

## Testing the Resilience Patterns

### 1. Basic Operations

```bash
# Store values
curl -X POST http://localhost:3000/foo -d "Hello, World!"
curl -X POST http://localhost:3000/bar -d "Resilience!"

# Retrieve values (goes through circuit breaker)
curl http://localhost:3000/foo
```

### 2. Health Checks (Normal State)

```bash
# Check readiness (circuit closed, returns 200)
curl -i http://localhost:3000/health/ready

# Check liveness (always returns 200)
curl http://localhost:3000/health/live

# View metrics
curl http://localhost:3000/metrics | jq
```

### 3. Chaos Engineering - Trip the Circuit Breaker

```bash
# Inject 80% failure rate
curl -X POST "http://localhost:3000/admin/chaos?rate=0.8"

# Make several GET requests to trigger failures
for i in {1..20}; do 
  curl http://localhost:3000/test$i 2>/dev/null
  echo ""
  sleep 0.1
done

# Check health again - circuit should now be OPEN (returns 503)
curl -i http://localhost:3000/health/ready

# View metrics showing failure counts
curl http://localhost:3000/metrics | jq '.circuit_breaker'
```

### 4. Recovery

```bash
# Reduce failure rate
curl -X POST "http://localhost:3000/admin/chaos?rate=0.1"

# Wait for circuit breaker to enter half-open state (5 seconds)
sleep 6

# Make successful requests to close the circuit
for i in {1..10}; do 
  curl http://localhost:3000/foo
  sleep 0.2
done

# Check health - should be back to 200
curl -i http://localhost:3000/health/ready
```

## Architecture

```
Client Request
     ↓
 Axum Router
     ↓
Circuit Breaker ← http_status() / health_status() (PR #121)
     ↓
 Chaos Layer ← Configurable failure injection
     ↓
Database Service (HashMap)
```

## Key Features Demonstrated

### Health Check Integration (PR #121)
- `http_status()` - Returns 200 (healthy) or 503 (degraded) based on circuit state
- `health_status()` - Returns "healthy", "degraded", or "unhealthy" string

### Chaos Engineering
- Dynamic failure rate configuration via `/admin/chaos?rate=X`
- Simulates database failures without modifying business logic
- Watch circuit breaker respond in real-time

### Circuit Breaker
- Threshold: 50% failure rate
- Window size: 10 calls
- Minimum calls: 5 (prevents premature tripping)
- Wait duration: 5 seconds in open state

## Kubernetes Deployment

```yaml
apiVersion: v1
kind: Pod
spec:
  containers:
  - name: kv-store
    image: axum-resilient-kv-store
    livenessProbe:
      httpGet:
        path: /health/live
        port: 3000
    readinessProbe:
      httpGet:
        path: /health/ready
        port: 3000
```

When the circuit breaker opens due to downstream failures, the readiness probe fails (503), and Kubernetes stops routing traffic to the pod until it recovers.
