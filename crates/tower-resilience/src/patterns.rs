//! # Pattern Guides
//!
//! Detailed guides for each resilience pattern, including when to use them, trade-offs,
//! real-world scenarios, and anti-patterns.

/// Circuit Breaker pattern guide
pub mod circuit_breaker {
    //! # Circuit Breaker
    //!
    //! Automatically stops calling a failing service to prevent cascading failures and give it
    //! time to recover.
    //!
    //! ## When to Use
    //!
    //! - **Failing downstream services**: When a dependency is experiencing issues
    //! - **Cascading failure prevention**: Stop failures from propagating through your system
    //! - **Graceful degradation**: Provide fallbacks when services are unavailable
    //! - **Load shedding**: Reduce load on struggling services
    //!
    //! ## Trade-offs
    //!
    //! - **Fail fast vs retry**: Circuit breaker fails immediately when open (combine with retry for best results)
    //! - **State overhead**: Requires tracking call history (~100-1000 calls)
    //! - **Tuning complexity**: Requires careful threshold configuration
    //! - **False positives**: May trip during legitimate traffic spikes
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Database Replica Failover
    //! ├─ Primary database becomes slow/unresponsive
    //! ├─ Circuit breaker opens after 50% failure rate
    //! ├─ Application switches to read replica
    //! └─ Periodic health checks test primary recovery
    //!
    //! External API Integration
    //! ├─ Third-party API rate limits or goes down
    //! ├─ Circuit opens to prevent timeout pile-up
    //! ├─ Fallback to cached data or degraded experience
    //! └─ Automatic recovery when API stabilizes
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Too aggressive thresholds**: Tripping on temporary blips
    //! ✅ Use minimum call counts and reasonable windows (e.g., 50% over 100 calls)
    //!
    //! ❌ **No fallback strategy**: Users see errors when circuit opens
    //! ✅ Provide cached data, default values, or graceful degradation
    //!
    //! ❌ **Using alone for retries**: Circuit breaker doesn't retry
    //! ✅ Combine with retry layer for transient failures
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "circuitbreaker")]
    //! # {
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let database_client = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let circuit_breaker = CircuitBreakerLayer::<(), std::io::Error>::builder()
    //!     .failure_rate_threshold(0.5)      // Open at 50% failures
    //!     .sliding_window_size(100)         // Over last 100 calls
    //!     .minimum_number_of_calls(10)      // Need at least 10 calls
    //!     .wait_duration_in_open(Duration::from_secs(30))  // Stay open 30s
    //!     .build();
    //!
    //! let service = circuit_breaker.layer::<_, ()>(database_client);
    //! # }
    //! # }
    //! ```
}

/// Bulkhead pattern guide
pub mod bulkhead {
    //! # Bulkhead
    //!
    //! Limits concurrent calls to isolate resources and prevent thread/connection pool
    //! exhaustion.
    //!
    //! ## When to Use
    //!
    //! - **Multi-tenant systems**: Prevent one tenant from consuming all resources
    //! - **Resource isolation**: Protect critical paths from expensive operations
    //! - **Thread pool exhaustion prevention**: Limit concurrent blocking operations
    //! - **Per-endpoint limits**: Prevent one slow endpoint from blocking others
    //!
    //! ## Trade-offs
    //!
    //! - **Resource utilization vs isolation**: Reserved capacity may be underutilized
    //! - **Queue depth management**: Waiting tasks consume memory
    //! - **Latency impact**: Requests may wait for permits
    //! - **Fairness**: No built-in priority mechanisms
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Multi-Tenant API
    //! ├─ Tenant A: Max 10 concurrent requests
    //! ├─ Tenant B: Max 10 concurrent requests
    //! ├─ Tenant A spike doesn't affect Tenant B
    //! └─ Fair resource allocation per tenant
    //!
    //! Worker Pool Management
    //! ├─ High-priority jobs: 20 workers
    //! ├─ Low-priority jobs: 5 workers
    //! ├─ Low-priority surge can't starve high-priority
    //! └─ Predictable resource usage
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Too many small bulkheads**: Management overhead exceeds benefits
    //! ✅ Bulkhead at service/tenant boundaries, not per-function
    //!
    //! ❌ **Not monitoring queue depth**: Memory exhaustion from waiting tasks
    //! ✅ Set `max_wait_duration` and monitor rejections
    //!
    //! ❌ **Using for rate limiting**: Bulkhead limits concurrency, not rate
    //! ✅ Use rate limiter for throughput limits
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "bulkhead")]
    //! # {
    //! use tower_resilience_bulkhead::BulkheadLayer;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let expensive_operation = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let bulkhead = BulkheadLayer::builder()
    //!     .max_concurrent_calls(10)
    //!     .max_wait_duration(Some(Duration::from_secs(5)))
    //!     .on_call_rejected(|max| {
    //!         eprintln!("Bulkhead exhausted (max: {})", max);
    //!     })
    //!     .build();
    //!
    //! let service = tower::ServiceBuilder::new()
    //!     .layer(bulkhead)
    //!     .service(expensive_operation);
    //! # }
    //! # }
    //! ```
}

/// Time Limiter pattern guide
pub mod time_limiter {
    //! # Time Limiter
    //!
    //! Enforces timeouts on operations with optional future cancellation.
    //!
    //! ## When to Use
    //!
    //! - **Unbounded operations**: Database queries, external APIs
    //! - **SLA enforcement**: Guarantee response times
    //! - **Resource protection**: Prevent long-running tasks from accumulating
    //! - **Circuit breaker complement**: Timeouts count as failures
    //!
    //! ## Trade-offs
    //!
    //! - **Cancellation semantics**: Dropping futures may not cancel underlying work
    //! - **Partial work cleanup**: Need to handle incomplete operations
    //! - **Timeout selection**: Too short causes false failures, too long defeats purpose
    //! - **Overhead**: Timer overhead for every call (~100ns)
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Database Query Timeout
    //! ├─ Query has 5s timeout
    //! ├─ Slow query triggers timeout
    //! ├─ Connection returned to pool (if cancel_running_future=true)
    //! └─ User sees timeout error instead of hanging
    //!
    //! External API Call
    //! ├─ API call has 10s timeout
    //! ├─ Network issue causes hang
    //! ├─ Timeout fires, request fails fast
    //! └─ Circuit breaker may open if timeouts are frequent
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Timeout too short**: Legitimate slow operations fail
    //! ✅ Set timeout to P99 latency + buffer
    //!
    //! ❌ **No cleanup on timeout**: Resources leak
    //! ✅ Use `cancel_running_future=true` when appropriate
    //!
    //! ❌ **Same timeout everywhere**: Different operations need different limits
    //! ✅ Configure per-endpoint or per-operation
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "timelimiter")]
    //! # {
    //! use tower_resilience::timelimiter::TimeLimiterLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let database_query = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let time_limiter = TimeLimiterLayer::builder()
    //!     .timeout_duration(Duration::from_secs(5))
    //!     .cancel_running_future(true)
    //!     .on_timeout(|| {
    //!         eprintln!("Query timeout");
    //!     })
    //!     .build();
    //!
    //! let service = tower::ServiceBuilder::new()
    //!     .layer(time_limiter)
    //!     .service(database_query);
    //! # }
    //! # }
    //! ```
}

/// Retry pattern guide
pub mod retry {
    //! # Retry
    //!
    //! Automatically retries failed operations with configurable backoff strategies.
    //!
    //! ## When to Use
    //!
    //! - **Transient failures**: Network blips, temporary resource unavailability
    //! - **Rate limiting**: 429 responses with retry-after
    //! - **Database deadlocks**: Transient conflicts
    //! - **Eventually consistent systems**: Retry until data is available
    //!
    //! ## Trade-offs
    //!
    //! - **Latency vs success rate**: Retries add latency but improve success
    //! - **Amplification effects**: Retries multiply load on failing services
    //! - **Idempotency requirements**: Safe retries require idempotent operations
    //! - **Jitter importance**: Without jitter, retries create thundering herd
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Network Transient Errors
    //! ├─ Connection reset by peer
    //! ├─ Retry with 100ms exponential backoff
    //! ├─ Success on 2nd attempt
    //! └─ User doesn't see error
    //!
    //! API Rate Limiting
    //! ├─ Receive 429 Too Many Requests
    //! ├─ Retry-After: 1s header
    //! ├─ Wait 1s + jitter
    //! └─ Retry succeeds
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Retrying non-idempotent operations**: Duplicate charges, double-sends
    //! ✅ Only retry GET, HEAD, PUT, DELETE; use idempotency keys for POST
    //!
    //! ❌ **No jitter**: All clients retry at same time (thundering herd)
    //! ✅ Use `exponential_backoff` with randomization
    //!
    //! ❌ **Infinite retries**: Never give up
    //! ✅ Set reasonable `max_attempts` (3-5)
    //!
    //! ❌ **Retrying 4xx errors**: Client errors won't succeed on retry
    //! ✅ Use retry predicate to only retry 5xx, network errors
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "retry")]
    //! # {
    //! use tower_resilience::retry::RetryLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let http_client = tower::service_fn(|_req: ()| async { Ok::<_, MyError>(()) });
    //! let retry = RetryLayer::<MyError>::builder()
    //!     .max_attempts(3)
    //!     .exponential_backoff(Duration::from_millis(100))
    //!     .retry_on(|err: &MyError| {
    //!         // Only retry transient errors
    //!         true  // Check if error is retryable
    //!     })
    //!     .build();
    //!
    //! let service = tower::ServiceBuilder::new()
    //!     .layer(retry)
    //!     .service(http_client);
    //! # }
    //! # }
    //! ```
}

/// Rate Limiter pattern guide
pub mod rate_limiter {
    //! # Rate Limiter
    //!
    //! Controls the rate of requests to protect downstream services and enforce quotas.
    //!
    //! ## When to Use
    //!
    //! - **Quota enforcement**: Per-user, per-tenant API limits
    //! - **Protecting resources**: Prevent overwhelming databases or APIs
    //! - **Fairness**: Ensure fair access to shared resources
    //! - **Cost control**: Limit expensive operations
    //!
    //! ## Trade-offs
    //!
    //! - **Throughput vs fairness**: Token bucket allows bursts
    //! - **Burst handling**: Should you allow temporary spikes?
    //! - **Rejection strategy**: Drop, queue, or return error?
    //! - **Distributed coordination**: Single-node vs multi-node limits
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Per-User API Limits
    //! ├─ Free tier: 100 req/min
    //! ├─ Pro tier: 1000 req/min
    //! ├─ Burst allowance for good UX
    //! └─ Return 429 when exceeded
    //!
    //! Downstream Protection
    //! ├─ Database has 1000 QPS limit
    //! ├─ Rate limit to 800 QPS (80% capacity)
    //! ├─ Prevents database overload
    //! └─ Predictable performance
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Global limits only**: One tenant can exhaust quota for all
    //! ✅ Per-tenant/per-user limits with global backstop
    //!
    //! ❌ **No burst allowance**: Poor user experience for spiky traffic
    //! ✅ Allow some burst (e.g., 2x rate for 1 second)
    //!
    //! ❌ **Using for concurrency limits**: Rate ≠ concurrency
    //! ✅ Use bulkhead for concurrency, rate limiter for throughput
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "ratelimiter")]
    //! # {
    //! use tower_resilience::ratelimiter::RateLimiterLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let api_handler = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let rate_limiter = RateLimiterLayer::builder()
    //!     .limit_for_period(100)                    // 100 requests
    //!     .refresh_period(Duration::from_secs(1))   // per second
    //!     .timeout_duration(Duration::from_millis(100))  // Wait up to 100ms
    //!     .build();
    //!
    //! let service = tower::ServiceBuilder::new()
    //!     .layer(rate_limiter)
    //!     .service(api_handler);
    //! # }
    //! # }
    //! ```
}

/// Cache pattern guide
pub mod cache {
    //! # Cache
    //!
    //! Caches responses to reduce load on expensive operations.
    //!
    //! ## When to Use
    //!
    //! - **Expensive computations**: Complex calculations, ML inference
    //! - **High read:write ratio**: Data changes infrequently
    //! - **Reducing load**: Protect databases or external APIs
    //! - **Latency optimization**: Serve cached responses faster
    //!
    //! ## Trade-offs
    //!
    //! - **Staleness vs load**: Fresh data vs reduced load
    //! - **Memory usage**: Cache size vs hit rate
    //! - **Cache invalidation**: "One of the two hard problems in CS"
    //! - **Cache stampede**: Thundering herd on cache miss
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! API Response Caching
    //! ├─ GET /users/{id} cached for 5 minutes
    //! ├─ First request: cache miss, query database
    //! ├─ Subsequent requests: cache hit, instant response
    //! └─ After 5 minutes: cache expires, refresh
    //!
    //! Computation Memoization
    //! ├─ Expensive report generation
    //! ├─ Cache result for 1 hour
    //! ├─ Multiple users see cached version
    //! └─ 95% reduction in computation load
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Caching errors**: Bad responses stay cached
    //! ✅ Only cache successful responses
    //!
    //! ❌ **No TTL**: Stale data served forever
    //! ✅ Set appropriate TTL based on data volatility
    //!
    //! ❌ **Cache stampede**: All requests miss simultaneously
    //! ✅ Use TTL jitter or request coalescing
    //!
    //! ❌ **Unbounded cache**: Memory exhaustion
    //! ✅ Set max_capacity with LRU eviction
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "cache")]
    //! # {
    //! use tower_resilience_cache::CacheLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Clone)]
    //! # struct Request { id: u64 }
    //! # async fn example() {
    //! # let expensive_operation = tower::service_fn(|_req: Request| async { Ok::<_, std::io::Error>(()) });
    //! let cache = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id)
    //!     .build();
    //!
    //! let service = tower::ServiceBuilder::new()
    //!     .layer(cache)
    //!     .service(expensive_operation);
    //! # }
    //! # }
    //! ```
}
