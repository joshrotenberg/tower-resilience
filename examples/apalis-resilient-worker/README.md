# Resilient Apalis workers

This example verifies that `tower-resilience` layers compose directly with an
[Apalis](https://apalis.dev/) 1.0 release-candidate worker. It runs three
finite, asserted scenarios against Apalis's in-memory backend:

- circuit-breaker backpressure pauses job intake while the circuit is open,
  then permits a half-open recovery call;
- a bulkhead caps concurrently spawned jobs without rejecting queued work;
- an AIMD adaptive limiter raises and lowers its concurrency limit from real
  job outcomes.

Run it with:

```console
cargo run -p apalis-resilient-worker
```

## Why the layers are attached to `WorkerBuilder`

Apalis already accepts Tower layers, so the integration point is its
`WorkerBuilder::layer` method. The layer sees Apalis's full `Task` request even
though the handler continues to receive the ergonomic job arguments.

Apalis task functions use a boxed dynamic service error. Bulkhead and adaptive
currently need the outer `map_err` adapter shown in the example because their
generic wrapper errors cannot convert back into that boxed type. This is
tracked in [#443](https://github.com/joshrotenberg/tower-resilience/issues/443);
the adapter intentionally makes the current integration executable, but loses
the original typed error and source chain.

For queue-backed work, backpressure is usually a better default than
fail-fast rejection. When a circuit is open or a bulkhead is full,
backpressure leaves pending jobs in the worker pipeline until capacity or the
dependency recovers. Fail-fast mode instead turns admission control into a job
error, which may consume retry attempts or trigger backend-specific failure
handling.

## Apalis overlap and tower-resilience additions

Apalis has built-in concurrency and circuit-breaker middleware. Its current
circuit breaker offers a consecutive-failure threshold, recovery timeout, and
half-open calls. `tower-resilience` is useful when an application also needs
features such as:

- selectable count-based or time-based failure-rate windows;
- response and custom failure classification;
- slow-call detection;
- named event listeners, tracing, metrics, and handles;
- explicit Tower readiness backpressure;
- the same policy types across workers, HTTP/gRPC clients, and other Tower
  services.

This example pins Apalis 1.0.0-rc.9. In that release, the built-in breaker's
[execution path uses literal default thresholds and open readiness returns
`Pending` without arranging a recovery wake](https://github.com/apalis-dev/apalis/blob/v1.0.0-rc.9/apalis-core/src/worker/ext/circuit_breaker/service.rs#L201-L266).
The circuit-breaker scenario here therefore also verifies configured thresholds
and timer-driven readiness recovery.

The two concurrency controls also operate at different levels. Apalis decides
how a worker spawns work; a bulkhead isolates a particular service or job type,
and an adaptive limiter changes its admission limit based on observed latency
and errors.

## Choose the protected boundary deliberately

Wrapping the whole worker classifies whole-job results. That is appropriate
when the job is the fault-isolation boundary. If a handler calls several
downstream systems, wrap each downstream Tower client instead so, for example,
a Stripe outage does not open the breaker for unrelated database work. A
handler that catches a downstream error and returns `Ok` also hides that error
from worker-level resilience middleware.
