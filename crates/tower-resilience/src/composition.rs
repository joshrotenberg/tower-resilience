//! # Composition Guide
//!
//! Comprehensive guide to composing resilience patterns together, including layer ordering,
//! error type integration, and workarounds for complex compositions.
//!
//! ## Note on Health Check
//!
//! Health Check is **not a Tower layer** and cannot be composed with other patterns in the
//! layer stack. Instead, it wraps resource instances at a different architectural level:
//!
//! ```text
//! Application Layer:
//!   HealthCheckWrapper (manages multiple resources)
//!     ├─ primary_db
//!     ├─ secondary_db
//!     └─ tertiary_db
//!
//! Tower Layer Stack:
//!   ServiceBuilder
//!     ├─ TimeLimiter
//!     ├─ CircuitBreaker
//!     ├─ Retry
//!     └─ Service (selected by HealthCheckWrapper)
//! ```
//!
//! Use Health Check to **select** healthy resources, then apply Tower layers to the
//! selected resource for request-level resilience.

/// Pattern selection guide - how to choose the right patterns
pub mod selection {
    //! # Pattern Selection Guide
    //!
    //! A comprehensive guide for selecting and composing resilience patterns.
    //!
    //! ## The Onion Model
    //!
    //! Tower middleware wraps services like layers of an onion. The outermost layer
    //! processes requests first (and responses last):
    //!
    //! ```text
    //! Request → [Retry] → [CircuitBreaker] → [Timeout] → [RateLimit] → Service
    //!                                                                     ↓
    //! Response ← [Retry] ← [CircuitBreaker] ← [Timeout] ← [RateLimit] ← Response
    //! ```
    //!
    //! **Key insight**: Layer order matters. A timeout outside retry limits total time;
    //! inside retry, each attempt gets the full timeout.
    //!
    //! ## Understanding ServiceBuilder Order
    //!
    //! `ServiceBuilder` applies layers **inside-out**: the first `.layer()` call wraps
    //! closest to the service (innermost), and the last `.layer()` call is outermost.
    //!
    //! ```text
    //! // Execution order: Fallback → Timeout → Retry → Service
    //! ServiceBuilder::new()
    //!     .layer(fallback)   // 3rd added, outermost, executes 1st
    //!     .layer(timeout)    // 2nd added, middle
    //!     .layer(retry)      // 1st added, innermost, executes last (closest to service)
    //!     .service(svc)
    //! ```
    //!
    //! ## Client-Side vs Server-Side Patterns
    //!
    //! | Context | Goal | Primary Patterns |
    //! |---------|------|------------------|
    //! | **Client-side** | Protect self from slow/failing dependencies | Retry, CircuitBreaker, Timeout, Hedge, Fallback |
    //! | **Server-side** | Protect self from overwhelming traffic | RateLimiter, Bulkhead, Timeout |
    //!
    //! Most applications need both: client-side patterns for outgoing calls,
    //! server-side patterns for incoming requests.
    //!
    //! ## Note on Health Check
    //!
    //! Health Check is **not a Tower layer** - it's a resource manager that wraps multiple
    //! backend instances (databases, cache nodes, etc.) and selects healthy ones. Use it
    //! alongside Tower layers, not as part of the layer stack. See the
    //! [module-level documentation](super) for details.
    //!
    //! ## Pattern Quick Reference
    //!
    //! | Pattern | Purpose | Use When | Avoid When |
    //! |---------|---------|----------|------------|
    //! | **Retry** | Recover from transient failures | Network blips, 503s, deadlocks | Non-idempotent ops, 4xx errors |
    //! | **CircuitBreaker** | Fail fast when dependency is down | Cascading failure risk | Single-shot operations |
    //! | **TimeLimiter** | Bound operation duration | Unbounded external calls | Already-bounded operations |
    //! | **Bulkhead** | Isolate resource pools | Multi-tenant, mixed criticality | Uniform workloads |
    //! | **RateLimiter** | Control throughput | API quotas, resource protection | Internal service calls |
    //! | **Fallback** | Graceful degradation | Cached alternatives exist | No sensible default |
    //! | **Hedge** | Reduce tail latency | Idempotent reads, P99 matters | Expensive operations |
    //! | **Cache** | Reduce load, improve latency | High read:write ratio | Frequently changing data |
    //! | **Adaptive** | Auto-tune concurrency | Unknown optimal limits | Known fixed capacity |
    //!
    //! ## Decision Flowchart
    //!
    //! Use this flowchart to determine which patterns to apply:
    //!
    //! ```text
    //! START: What are you protecting?
    //! │
    //! ├─► Calling external dependency?
    //! │   │
    //! │   ├─► Can fail transiently? ────────────► Add Retry
    //! │   │
    //! │   ├─► Can hang indefinitely? ───────────► Add TimeLimiter
    //! │   │
    //! │   ├─► Can be completely down? ──────────► Add CircuitBreaker
    //! │   │
    //! │   ├─► Has rate limits? ─────────────────► Add RateLimiter (client-side)
    //! │   │
    //! │   ├─► P99 latency critical? ────────────► Add Hedge (if idempotent)
    //! │   │
    //! │   └─► Has fallback data? ───────────────► Add Fallback
    //! │
    //! ├─► Receiving incoming requests?
    //! │   │
    //! │   ├─► Multi-tenant? ────────────────────► Add Bulkhead (per-tenant)
    //! │   │
    //! │   ├─► Need to limit throughput? ────────► Add RateLimiter
    //! │   │
    //! │   ├─► Mixed criticality? ───────────────► Add Bulkhead (per-priority)
    //! │   │
    //! │   └─► Unknown optimal concurrency? ─────► Add AdaptiveLimiter
    //! │
    //! └─► Caching opportunity?
    //!     │
    //!     ├─► High read:write ratio? ───────────► Add Cache
    //!     │
    //!     └─► Concurrent duplicate requests? ───► Add Coalesce
    //! ```
}

/// Progressive stacks for different service types
pub mod stacks {
    //! # Progressive Stacks by Service Type
    //!
    //! Ready-to-use resilience configurations for common scenarios.
    //!
    //! ## External API Clients
    //!
    //! For calling third-party APIs (Stripe, Twilio, AWS, etc.):
    //!
    //! **Minimal Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Bound total time
    //!     .layer(RetryLayer::new(config))                         // Retry transient failures
    //!     .service(http_client)
    //! ```
    //!
    //! **Standard Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Outermost: total budget
    //!     .layer(RetryLayer::builder()                            // Retry within budget
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .layer(CircuitBreakerLayer::builder()                   // Track failures per-attempt
    //!         .failure_rate_threshold(0.5)
    //!         .build())
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Innermost: per-attempt limit
    //!     .service(http_client)
    //! ```
    //!
    //! **Full Stack (with fallback):**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(FallbackLayer::value(cached_response))           // Outermost: catch all failures
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Total budget for all retries
    //!     .layer(RetryLayer::builder()                            // Retry before CB sees failure
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .layer(CircuitBreakerLayer::builder()                   // Track per-attempt failures
    //!         .failure_rate_threshold(0.5)
    //!         .wait_duration_in_open(Duration::from_secs(30))
    //!         .build())
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Innermost: per-attempt limit
    //!     .service(http_client)
    //! ```
    //!
    //! **With Hedging (for latency-sensitive, idempotent calls):**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Total budget
    //!     .layer(RetryLayer::builder()                            // Retry failures
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .layer(CircuitBreakerLayer::builder()                   // Track failures
    //!         .failure_rate_threshold(0.5)
    //!         .build())
    //!     .layer(HedgeLayer::builder()                            // Fire backup after P95
    //!         .delay(Duration::from_millis(50))
    //!         .max_hedged_attempts(2)
    //!         .build())
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Per-attempt limit
    //!     .service(api_client)
    //! ```
    //!
    //! ## Database Connections
    //!
    //! For database clients (PostgreSQL, MySQL, etc.):
    //!
    //! **Standard Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Bound query time
    //!     .layer(RetryLayer::builder()                            // Retry transient errors
    //!         .max_attempts(2)
    //!         .retry_on(|e| is_transient_db_error(e))
    //!         .build())
    //!     .layer(BulkheadLayer::builder()                         // Limit concurrent queries
    //!         .max_concurrent_calls(20)  // Match connection pool size
    //!         .build())
    //!     .service(db_client)
    //! ```
    //!
    //! **With Circuit Breaker (for replicas):**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Bound query time
    //!     .layer(CircuitBreakerLayer::builder()                   // Fail fast if replica down
    //!         .failure_rate_threshold(0.5)
    //!         .minimum_number_of_calls(10)
    //!         .build())
    //!     .layer(BulkheadLayer::builder()                         // Limit concurrent queries
    //!         .max_concurrent_calls(20)
    //!         .build())
    //!     .service(db_client)
    //! ```
    //!
    //! ## Internal Microservices
    //!
    //! For calling other services you control:
    //!
    //! **Standard Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Bound total time
    //!     .layer(RetryLayer::builder()                            // Retry transient failures
    //!         .max_attempts(2)
    //!         .fixed_backoff(Duration::from_millis(50))
    //!         .build())
    //!     .layer(CircuitBreakerLayer::builder()                   // Fail fast if service down
    //!         .failure_rate_threshold(0.6)
    //!         .slow_call_rate_threshold(0.8)
    //!         .slow_call_duration_threshold(Duration::from_secs(2))
    //!         .build())
    //!     .service(grpc_client)
    //! ```
    //!
    //! **With Adaptive Concurrency:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Bound total time
    //!     .layer(AdaptiveLimiterLayer::new(Vegas::default()))     // Auto-tune concurrency
    //!     .layer(RetryLayer::builder()                            // Retry transient failures
    //!         .max_attempts(2)
    //!         .build())
    //!     .service(grpc_client)
    //! ```
    //!
    //! ## Latency-Critical Paths
    //!
    //! For operations where P99 latency matters (trading, real-time):
    //!
    //! **With Hedging:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_millis(100)))  // Tight deadline
    //!     .layer(HedgeLayer::builder()                               // Fire hedge after 10ms
    //!         .delay(Duration::from_millis(10))
    //!         .max_hedged_attempts(2)
    //!         .build())
    //!     .service(cache_client)
    //! ```
    //!
    //! **Parallel Hedging (fire all immediately):**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_millis(50)))   // Very tight deadline
    //!     .layer(HedgeLayer::builder()                               // Race all regions
    //!         .no_delay()  // Fire all requests immediately
    //!         .max_hedged_attempts(3)
    //!         .build())
    //!     .service(multi_region_client)
    //! ```
    //!
    //! ## Message Queues
    //!
    //! For Kafka, RabbitMQ, SQS, etc.:
    //!
    //! **Consumer Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Bound processing time
    //!     .layer(RetryLayer::builder()                            // Retry with backoff
    //!         .max_attempts(5)
    //!         .exponential_backoff(Duration::from_secs(1))
    //!         .max_backoff(Duration::from_secs(60))
    //!         .build())
    //!     .layer(CircuitBreakerLayer::builder()                   // Fail fast if handler broken
    //!         .failure_rate_threshold(0.5)
    //!         .wait_duration_in_open(Duration::from_secs(60))
    //!         .build())
    //!     .service(message_handler)
    //! ```
    //!
    //! **Producer Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Bound publish time
    //!     .layer(RetryLayer::builder()                            // Retry transient failures
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .layer(BulkheadLayer::builder()                         // Limit concurrent publishes
    //!         .max_concurrent_calls(50)
    //!         .build())
    //!     .service(queue_producer)
    //! ```
    //!
    //! ## Caching Layers
    //!
    //! For Redis, Memcached, etc.:
    //!
    //! **Standard Stack:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(FallbackLayer::value(None))                        // Outermost: cache miss is OK
    //!     .layer(TimeLimiterLayer::new(Duration::from_millis(50)))  // Fast timeout
    //!     .layer(CircuitBreakerLayer::builder()                     // Fail fast if cache down
    //!         .failure_rate_threshold(0.3)                          // Sensitive threshold
    //!         .build())
    //!     .service(redis_client)
    //! ```
    //!
    //! **With Request Coalescing:**
    //! ```text
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_millis(100)))  // Bound lookup time
    //!     .layer(CoalesceLayer::new())                               // Dedupe concurrent lookups
    //!     .layer(CircuitBreakerLayer::builder()                      // Fail fast if cache down
    //!         .failure_rate_threshold(0.3)
    //!         .build())
    //!     .service(redis_client)
    //! ```
}

/// Common composition patterns
pub mod patterns {
    //! # Composition Patterns
    //!
    //! Patterns are designed to be composed together for comprehensive resilience.
    //!
    //! ## Inbound (Server-Side)
    //!
    //! Protect your service from abusive or overwhelming clients:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │  Rate Limiter   │ ← Reject abusive clients
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │    Bulkhead     │ ← Isolate tenant resources
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Prevent runaway requests
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │     Handler     │
    //! └─────────────────┘
    //! ```
    //!
    //! ## Outbound (Client-Side)
    //!
    //! Make your clients resilient to downstream failures:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │    Fallback     │ ← Graceful degradation
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Don't wait forever
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │ Circuit Breaker │ ← Fail fast when down
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │      Retry      │ ← Handle transient errors
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │    Reconnect    │ ← Auto-reconnect on disconnect
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │     Client      │
    //! └─────────────────┘
    //! ```
    //!
    //! ## Read-Through Cache
    //!
    //! Cache expensive operations with resilience:
    //!
    //! ```text
    //! ┌─────────────┐
    //! │   Request   │
    //! └──────┬──────┘
    //!        │
    //!        ▼
    //! ┌─────────────────┐
    //! │      Cache      │ ← Try cache first
    //! └────────┬────────┘
    //!          │ (miss)
    //!          ▼
    //! ┌─────────────────┐
    //! │ Circuit Breaker │ ← Protect backend
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │  Time Limiter   │ ← Bound latency
    //! └────────┬────────┘
    //!          │
    //!          ▼
    //! ┌─────────────────┐
    //! │    Backend      │
    //! └─────────────────┘
    //! ```
}

/// Layer ordering guide
pub mod ordering {
    //! # Layer Ordering
    //!
    //! Layer order is critical! Understanding how `ServiceBuilder` applies layers
    //! is essential for correct composition.
    //!
    //! ## ServiceBuilder Order
    //!
    //! `ServiceBuilder` applies layers **inside-out**: the first `.layer()` call wraps
    //! closest to the service (innermost), and the last `.layer()` call is outermost.
    //!
    //! ```text
    //! // Build order vs Execution order:
    //! ServiceBuilder::new()
    //!     .layer(A)  // Added 1st → Innermost  → Executes LAST  (closest to service)
    //!     .layer(B)  // Added 2nd → Middle     → Executes 2nd
    //!     .layer(C)  // Added 3rd → Outermost  → Executes FIRST (sees request first)
    //!     .service(svc)
    //!
    //! // Execution: Request → C → B → A → Service → A → B → C → Response
    //! ```
    //!
    //! ## Recommended Layer Order
    //!
    //! From outermost (first to process request) to innermost:
    //!
    //! | Position | Layer | Rationale | If Wrong |
    //! |----------|-------|-----------|----------|
    //! | 1 | Fallback | Catch all errors | Users see raw errors |
    //! | 2 | Total Timeout | Bound entire operation | Can wait forever across retries |
    //! | 3 | Retry | Retry within timeout budget | Retries exceed total timeout |
    //! | 4 | Circuit Breaker | Fail fast when down | Wastes retries on dead service |
    //! | 5 | Bulkhead | Limit concurrency | Slow calls exhaust resources before CB trips |
    //! | 6 | Per-call Timeout | Bound each attempt | One slow call consumes retry budget |
    //! | 7 | Rate Limiter | Respect downstream limits | Retry amplifies rate limit violations |
    //! | 8 | Hedge | Fire parallel requests | Hedges not bounded by per-call timeout |
    //! | 9 | Service | The actual service | - |
    //!
    //! ## Client-Side (Outbound) - Correct Order
    //!
    //! In `ServiceBuilder`, add layers from innermost to outermost:
    //!
    //! ```text
    //! ServiceBuilder::new()
    //!     // Added last → outermost → executes first
    //!     .layer(fallback)           // Catches all errors, provides degraded response
    //!     .layer(cache)              // Skips remaining layers on cache hit
    //!     .layer(total_timeout)      // Bounds entire operation including retries
    //!     // Middle layers
    //!     .layer(circuit_breaker)    // Fails fast if service is down
    //!     .layer(retry)              // Retries individual failures
    //!     // Added first → innermost → executes last (closest to service)
    //!     .layer(per_call_timeout)   // Bounds each attempt
    //!     .service(http_client);
    //! ```
    //!
    //! ## Server-Side (Inbound) - Correct Order
    //!
    //! ```text
    //! ServiceBuilder::new()
    //!     // Added last → outermost → executes first
    //!     .layer(rate_limiter)       // Reject over-limit requests immediately
    //!     .layer(bulkhead)           // Isolate resources after rate limiting
    //!     // Added first → innermost → executes last
    //!     .layer(timeout)            // Bound handler execution
    //!     .service(handler);
    //! ```
    //!
    //! ## Common Patterns
    //!
    //! **Timeout inside vs outside Retry:**
    //!
    //! ```text
    //! // Total timeout (all retries must complete in 30s)
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Outermost
    //!     .layer(RetryLayer::new(config))
    //!     .service(svc)
    //!
    //! // Per-attempt timeout (each attempt gets 10s)
    //! ServiceBuilder::new()
    //!     .layer(RetryLayer::new(config))
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Innermost
    //!     .service(svc)
    //!
    //! // Both (recommended for external APIs)
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Total (outer)
    //!     .layer(RetryLayer::new(config))
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Per-attempt (inner)
    //!     .service(svc)
    //! ```
    //!
    //! **Circuit Breaker with Retry:**
    //!
    //! Both orderings are valid with different semantics:
    //!
    //! ```text
    //! // CB outside retry (recommended for most cases):
    //! // - Retries happen first, CB only sees final result
    //! // - CB counts "did all retries fail?" not "did one attempt fail?"
    //! // - Fewer CB state transitions
    //! ServiceBuilder::new()
    //!     .layer(CircuitBreakerLayer::new(cb_config))  // Outer
    //!     .layer(RetryLayer::new(config))              // Inner
    //!     .service(svc)
    //!
    //! // CB inside retry:
    //! // - CB can open mid-retry, failing fast on subsequent attempts
    //! // - More responsive to cascading failures
    //! // - Each retry attempt is a separate CB call
    //! ServiceBuilder::new()
    //!     .layer(RetryLayer::new(config))              // Outer
    //!     .layer(CircuitBreakerLayer::new(cb_config))  // Inner
    //!     .service(svc)
    //! ```
}

/// Common anti-patterns to avoid
pub mod anti_patterns {
    //! # Common Anti-Patterns
    //!
    //! Pitfalls to avoid when composing resilience patterns.
    //!
    //! ## 1. Retry Storm
    //!
    //! **Problem:** Retrying without backoff overwhelms failing services.
    //!
    //! ```text
    //! // Bad: Immediate retries hammer the service
    //! RetryLayer::builder()
    //!     .max_attempts(5)
    //!     .no_backoff()
    //!     .build()
    //! ```
    //!
    //! ```text
    //! // Good: Exponential backoff with jitter
    //! RetryLayer::builder()
    //!     .max_attempts(3)
    //!     .exponential_backoff(Duration::from_millis(100))
    //!     .build()
    //! ```
    //!
    //! ## 2. Timeout Inside Timeout
    //!
    //! **Problem:** Inner timeout longer than outer, making outer ineffective.
    //!
    //! ```text
    //! // Bad: Inner timeout (10s) longer than outer (5s) - inner never triggers
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(5)))   // Outer: 5s
    //!     .layer(RetryLayer::new(config))
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Inner: 10s
    //!     .service(svc)
    //! ```
    //!
    //! ```text
    //! // Good: Inner timeout shorter than outer
    //! ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(30)))  // Total: 30s
    //!     .layer(RetryLayer::new(config))
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))  // Per-attempt: 10s
    //!     .service(svc)
    //! ```
    //!
    //! ## 3. Circuit Breaker Too Sensitive
    //!
    //! **Problem:** Opens on normal traffic variance.
    //!
    //! ```text
    //! // Bad: Opens on 2 failures out of 10
    //! CircuitBreakerLayer::builder()
    //!     .failure_rate_threshold(0.1)  // 10% threshold
    //!     .minimum_number_of_calls(10)
    //!     .build()
    //! ```
    //!
    //! ```text
    //! // Good: Requires sustained failure pattern
    //! CircuitBreakerLayer::builder()
    //!     .failure_rate_threshold(0.5)   // 50% threshold
    //!     .minimum_number_of_calls(20)   // Need 20 calls before evaluating
    //!     .sliding_window_size(100)      // Over last 100 calls
    //!     .build()
    //! ```
    //!
    //! ## 4. Bulkhead Too Small
    //!
    //! **Problem:** Legitimate traffic rejected.
    //!
    //! ```text
    //! // Bad: Only 2 concurrent calls
    //! BulkheadLayer::builder()
    //!     .max_concurrent_calls(2)
    //!     .build()
    //! ```
    //!
    //! ```text
    //! // Good: Sized for expected concurrency + headroom
    //! BulkheadLayer::builder()
    //!     .max_concurrent_calls(50)  // Based on load testing
    //!     .max_wait_duration(Duration::from_secs(1))
    //!     .build()
    //! ```
    //!
    //! ## 5. Retrying Non-Idempotent Operations
    //!
    //! **Problem:** Duplicate side effects (double charges, duplicate messages).
    //!
    //! ```text
    //! // Bad: Retry POST without idempotency
    //! RetryLayer::builder()
    //!     .retry_on(|_| true)  // Retries everything
    //!     .build()
    //! ```
    //!
    //! ```text
    //! // Good: Only retry safe operations
    //! RetryLayer::builder()
    //!     .retry_on(|e| {
    //!         e.is_transient() && request.method().is_idempotent()
    //!     })
    //!     .build()
    //! ```
    //!
    //! ## 6. Missing Fallback for User-Facing Services
    //!
    //! **Problem:** Users see raw errors.
    //!
    //! ```text
    //! // Bad: Error propagates to user
    //! ServiceBuilder::new()
    //!     .layer(CircuitBreakerLayer::new(config))
    //!     .service(product_catalog)
    //! ```
    //!
    //! ```text
    //! // Good: Graceful degradation
    //! ServiceBuilder::new()
    //!     .layer(FallbackLayer::value(cached_catalog))
    //!     .layer(CircuitBreakerLayer::new(config))
    //!     .service(product_catalog)
    //! ```
    //!
    //! ## 7. Hedging Non-Idempotent Operations
    //!
    //! **Problem:** Duplicate writes or side effects.
    //!
    //! ```text
    //! // Bad: Hedging a write operation
    //! ServiceBuilder::new()
    //!     .layer(HedgeLayer::builder()
    //!         .delay(Duration::from_millis(50))
    //!         .build())
    //!     .service(create_order)  // Could create duplicate orders!
    //! ```
    //!
    //! ```text
    //! // Good: Only hedge reads or idempotent operations
    //! ServiceBuilder::new()
    //!     .layer(HedgeLayer::builder()
    //!         .delay(Duration::from_millis(50))
    //!         .build())
    //!     .service(get_product)  // Safe to duplicate
    //! ```
    //!
    //! ## 8. Retry Without Timeout
    //!
    //! **Problem:** Each attempt can hang forever, exhausting resources.
    //!
    //! ```text
    //! // Bad: No timeout, retries can hang indefinitely
    //! ServiceBuilder::new()
    //!     .layer(RetryLayer::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .service(http_client)
    //! ```
    //!
    //! ```text
    //! // Good: Timeout bounds each attempt
    //! ServiceBuilder::new()
    //!     .layer(RetryLayer::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .layer(TimeLimiterLayer::new(Duration::from_secs(10)))
    //!     .service(http_client)
    //! ```
}

/// Error type integration strategies
pub mod error_types {
    //! # Error Type Integration
    //!
    //! When composing multiple resilience layers, all layers must agree on error types.
    //! Tower-resilience provides three approaches, from simplest to most flexible.
    //!
    //! ## 1. `ResilienceError<E>` (Recommended - Zero Boilerplate)
    //!
    //! Use the provided [`ResilienceError<E>`](tower_resilience_core::ResilienceError) type
    //! to eliminate manual `From` implementations:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "bulkhead", feature = "ratelimiter"))]
    //! # {
    //! use tower::ServiceBuilder;
    //! use tower_resilience_core::ResilienceError;
    //! use tower_resilience_bulkhead::BulkheadLayer;
    //! use tower_resilience::ratelimiter::RateLimiterLayer;
    //! use std::time::Duration;
    //!
    //! // Your application error
    //! #[derive(Debug, Clone)]
    //! enum AppError {
    //!     DatabaseDown,
    //!     InvalidRequest,
    //! }
    //!
    //! impl std::fmt::Display for AppError {
    //!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    //!         match self {
    //!             AppError::DatabaseDown => write!(f, "Database down"),
    //!             AppError::InvalidRequest => write!(f, "Invalid"),
    //!         }
    //!     }
    //! }
    //!
    //! impl std::error::Error for AppError {}
    //!
    //! // That's it! Zero From implementations needed
    //! type ServiceError = ResilienceError<AppError>;
    //!
    //! // All resilience layer errors automatically convert to ResilienceError
    //! // let service = ServiceBuilder::new()
    //! //     .layer(bulkhead)
    //! //     .layer(rate_limiter)
    //! //     .service(my_service);
    //! # }
    //! ```
    //!
    //! **Benefits:**
    //! - Zero boilerplate - no manual `From` implementations
    //! - Works with any number of layers
    //! - Rich error context (layer names, counts, durations)
    //! - Convenient helpers: `is_timeout()`, `is_rate_limited()`, etc.
    //! - Application errors wrapped in `Application` variant
    //!
    //! **Use when:**
    //! - Building new services
    //! - You want to move fast with minimal code
    //! - Standard error categorization is sufficient
    //!
    //! ## 2. Custom Error Type with Manual From
    //!
    //! Define your own error type and implement `From` for each layer:
    //!
    //! ```rust,no_run
    //! # use std::time::Duration;
    //! # #[cfg(all(feature = "retry", feature = "circuitbreaker"))]
    //! # {
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //!
    //! #[derive(Debug, Clone)]
    //! enum ServiceError {
    //!     Network(String),
    //!     Timeout,
    //!     CircuitOpen,
    //!     RateLimit,
    //! }
    //!
    //! // Manual From implementations give you full control
    //! // impl From<BulkheadError> for ServiceError { /* ... */ }
    //! // impl From<CircuitBreakerError> for ServiceError { /* ... */ }
    //!
    //! let retry = RetryLayer::<(), ServiceError>::builder()
    //!     .max_attempts(3)
    //!     .retry_on(|err| matches!(err, ServiceError::Network(_)))
    //!     .build();
    //! # }
    //! ```
    //!
    //! **Use when:**
    //! - You need very specific error semantics
    //! - Different recovery strategies per layer
    //! - Integrating with legacy error types
    //! - Custom error logging requirements
    //!
    //! ## 3. Error Mapping Layer
    //!
    //! Use `tower::util::MapErr` to convert between error types:
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "retry")]
    //! # {
    //! use tower::{ServiceBuilder, ServiceExt};
    //! use tower_resilience::retry::RetryLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug)]
    //! # struct DatabaseError;
    //! # #[derive(Debug, Clone)]
    //! # struct AppError;
    //! # impl From<DatabaseError> for AppError {
    //! #     fn from(_: DatabaseError) -> Self { AppError }
    //! # }
    //! # async fn example() {
    //! # let db_service = tower::service_fn(|_req: ()| async { Ok::<_, DatabaseError>(()) });
    //! let service = ServiceBuilder::new()
    //!     .layer(RetryLayer::<(), AppError>::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .map_err(|err: DatabaseError| AppError::from(err))
    //!     .service(db_service);
    //! # }
    //! # }
    //! ```
}

/// Advanced composition techniques
pub mod advanced {
    //! # Advanced Composition
    //!
    //! ## Overview
    //!
    //! Tower-resilience patterns are designed to compose together using Tower's `ServiceBuilder`.
    //! However, composing 3+ layers can encounter Rust trait bound limitations. This guide
    //! explains successful patterns and workarounds.
    //!
    //! ## Basic Composition (2 Layers)
    //!
    //! Two-layer composition works reliably with `ServiceBuilder`:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter"))]
    //! # {
    //! use tower::ServiceBuilder;
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let service = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
    //! let composed = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::<()>::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .layer(RetryLayer::<(), MyError>::builder()
    //!         .max_attempts(3)
    //!         .exponential_backoff(Duration::from_millis(100))
    //!         .build())
    //!     .service(service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Limitations with 3+ Layers
    //!
    //! **Problem**: Composing 3+ resilience layers using `ServiceBuilder` often hits Rust
    //! trait bound limitations. This is a known issue with complex Tower layer stacks.
    //!
    //! **Why it happens**:
    //! - Each layer wraps the service in a new type
    //! - Trait bounds become deeply nested
    //! - Rust's type inference struggles with complex layer stacks
    //! - Some combinations trigger "overflow evaluating the requirement" errors
    //!
    //! **Example that may fail**:
    //!
    //! ```rust,ignore
    //! // This may encounter trait bound errors
    //! ServiceBuilder::new()
    //!     .layer(cache_layer)
    //!     .layer(circuit_breaker)
    //!     .layer(retry_layer)
    //!     .layer(timeout_layer)
    //!     .service(base_service);  // Error: trait bounds not satisfied
    //! ```
    //!
    //! ## Workarounds
    //!
    //! ### 1. Manual Layer Composition
    //!
    //! Apply layers one at a time manually (most reliable):
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "circuitbreaker", feature = "cache"))]
    //! # {
    //! use tower::Layer;
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //! use tower_resilience_cache::CacheLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # #[derive(Clone)]
    //! # struct Request { id: u64 }
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|req: Request| async { Ok::<_, MyError>(req) });
    //! // Build layers inside-out manually
    //! let with_retry = RetryLayer::<Request, MyError>::builder()
    //!     .max_attempts(3)
    //!     .build()
    //!     .layer(base_service);
    //!
    //! let with_circuit_breaker = CircuitBreakerLayer::<Request, MyError>::builder()
    //!     .failure_rate_threshold(0.5)
    //!     .build()
    //!     .layer(with_retry);
    //!
    //! let service = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id)
    //!     .build()
    //!     .layer(with_circuit_breaker);
    //! # }
    //! # }
    //! ```
    //!
    //! ### 2. Limit ServiceBuilder Stack Depth
    //!
    //! Keep ServiceBuilder stacks to 2-3 layers max, compose manually beyond that:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter", feature = "cache"))]
    //! # {
    //! use tower::{ServiceBuilder, Layer};
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use tower_resilience_cache::CacheLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # #[derive(Clone)]
    //! # struct Request { id: u64 }
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|req: Request| async { Ok::<_, MyError>(req) });
    //! // First 2 layers via ServiceBuilder
    //! let inner = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::<Request>::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .layer(RetryLayer::<Request, MyError>::builder()
    //!         .max_attempts(3)
    //!         .build())
    //!     .service(base_service);
    //!
    //! // Additional layers manually
    //! let service = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id)
    //!     .build()
    //!     .layer(inner);
    //! # }
    //! # }
    //! ```
    //!
    //! ### 3. Split Complex Compositions
    //!
    //! For very complex stacks, split into logical groups and compose separately:
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "retry", feature = "timelimiter"))]
    //! # {
    //! use tower::{ServiceBuilder, Layer};
    //! use tower_resilience::retry::RetryLayer;
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let base_service = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
    //! // Build retry layer first
    //! let retry_layer = RetryLayer::<(), MyError>::builder()
    //!     .max_attempts(3)
    //!     .build();
    //!
    //! // Apply retry manually
    //! let with_retry = retry_layer.layer(base_service);
    //!
    //! // Then use ServiceBuilder for remaining layers
    //! let service = ServiceBuilder::new()
    //!     .layer(TimeLimiterLayer::<()>::builder()
    //!         .timeout_duration(Duration::from_secs(5))
    //!         .build())
    //!     .service(with_retry);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Working Examples
    //!
    //! See the repository examples for complete, working compositions:
    //!
    //! - [`examples/composition_outbound.rs`] - Client-side resilience stack
    //! - [`examples/server_api.rs`] - Server-side protection
    //! - [`examples/database_client.rs`] - Database client with retry + circuit breaker
    //! - [`examples/message_queue_worker.rs`] - Message processing with bulkhead + retry
    //!
    //! [`examples/composition_outbound.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/composition_outbound.rs
    //! [`examples/server_api.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/server_api.rs
    //! [`examples/database_client.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/database_client.rs
    //! [`examples/message_queue_worker.rs`]: https://github.com/joshrotenberg/tower-resilience/blob/main/examples/message_queue_worker.rs
}

/// References and further reading
pub mod references {
    //! # References
    //!
    //! ## Books
    //!
    //! - **"Release It!" by Michael Nygard** - Foundational text on stability patterns
    //!
    //! ## Papers
    //!
    //! - **"The Tail at Scale" (Dean & Barroso, Google 2013)** - Essential reading on
    //!   tail latency and hedging.
    //!   [CACM Link](https://cacm.acm.org/research/the-tail-at-scale/)
    //!
    //! ## Prior Art
    //!
    //! - **Resilience4j** (Java) - Primary inspiration for this library.
    //!   [Docs](https://resilience4j.readme.io/)
    //! - **Polly** (.NET) - Excellent PolicyWrap ordering guide.
    //!   [Wiki](https://github.com/App-vNext/Polly/wiki/PolicyWrap)
    //! - **Netflix Hystrix** (maintenance mode) - Pioneered many patterns.
    //!   [GitHub](https://github.com/Netflix/Hystrix)
    //! - **Failsafe** (Java) - Clean API design inspiration.
    //!   [Docs](https://failsafe.dev/)
    //!
    //! ## Tower Ecosystem
    //!
    //! - **tower** - [docs.rs/tower](https://docs.rs/tower)
    //! - **tower-http** - [docs.rs/tower-http](https://docs.rs/tower-http)
    //!
    //! ## Pattern Documentation
    //!
    //! - [Circuit Breaker Pattern (Martin Fowler)](https://martinfowler.com/bliki/CircuitBreaker.html)
    //! - [Bulkhead Pattern (Azure Architecture)](https://docs.microsoft.com/en-us/azure/architecture/patterns/bulkhead)
    //! - [Retry Pattern (Azure Architecture)](https://docs.microsoft.com/en-us/azure/architecture/patterns/retry)
}
