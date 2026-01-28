//! # Pattern Guides
//!
//! Detailed guides for each resilience pattern, including when to use them, trade-offs,
//! real-world scenarios, and anti-patterns.
//!
//! ## Available Patterns
//!
//! - [Circuit Breaker](circuit_breaker) - Stop calling failing services
//! - [Bulkhead](bulkhead) - Isolate resources with concurrency limits
//! - [Time Limiter](time_limiter) - Enforce operation timeouts
//! - [Retry](retry) - Retry transient failures with backoff
//! - [Rate Limiter](rate_limiter) - Control request throughput
//! - [Cache](cache) - Memoize expensive operations
//! - [Reconnect](reconnect) - Auto-reconnect persistent connections
//! - [Health Check](healthcheck) - Proactive resource health monitoring
//! - [Fallback](fallback) - Provide alternative responses on failure
//! - [Hedge](hedge) - Reduce tail latency with parallel requests
//! - [Executor](executor) - Delegate request processing to dedicated executors
//! - [Adaptive Concurrency](adaptive) - Dynamic concurrency limiting with AIMD/Vegas
//! - [Coalesce](coalesce) - Deduplicate concurrent identical requests (singleflight)

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

/// Health Check pattern guide
pub mod healthcheck {
    //! # Health Check
    //!
    //! Proactive health monitoring for resources with intelligent selection strategies.
    //! Continuously checks resource health in the background and provides access to
    //! healthy resources on demand.
    //!
    //! ## Health Check vs Circuit Breaker
    //!
    //! **Key distinction**: Health Check is **proactive**, Circuit Breaker is **reactive**.
    //!
    //! - **Health Check**: Monitors resources *before* use, prevents failures
    //! - **Circuit Breaker**: Responds *after* failures happen, limits damage
    //!
    //! These patterns **complement each other perfectly**:
    //! - Health Check layer selects healthy resources
    //! - Circuit Breaker layer protects against cascading failures
    //!
    //! ## When to Use
    //!
    //! ✅ **Multiple resource instances**: Primary/secondary databases, regional endpoints
    //! ✅ **Automatic failover**: Switch to healthy resources without manual intervention
    //! ✅ **Load distribution**: Round-robin or weighted selection across healthy instances
    //! ✅ **Kubernetes readiness**: Export health status for K8s probes
    //!
    //! ❌ **Single resource**: Use Circuit Breaker instead
    //! ❌ **Request-level failures**: Use Retry layer
    //! ❌ **Middleware composition**: Health Check is not a Tower layer
    //!
    //! ## Design Philosophy
    //!
    //! Health Check is **not a Tower layer** - it's a wrapper pattern that manages multiple
    //! resources:
    //!
    //! ```text
    //! Tower Layers (middleware):        Health Check (resource manager):
    //!   Request → Retry →                  ┌─────────────────┐
    //!             CircuitBreaker →         │  Health Wrapper │
    //!             Service                  │  - primary ✓    │
    //!                                      │  - secondary ✓  │
    //!                                      │  - tertiary ✗   │
    //!                                      └─────────────────┘
    //!                                            ↓
    //!                                      Select healthy resource
    //! ```
    //!
    //! ## Selection Strategies
    //!
    //! ### FirstAvailable (Default)
    //! Returns the first healthy resource. Best for primary/secondary failover.
    //!
    //! ### RoundRobin
    //! Distributes load evenly across healthy resources.
    //!
    //! ### Random
    //! Randomly selects from healthy resources (requires `random` feature).
    //!
    //! ### PreferHealthy
    //! Prefers fully healthy resources, falls back to degraded if needed.
    //!
    //! ### Custom
    //! Implement custom logic (latency-based, geographic proximity, weighted, etc.).
    //!
    //! ## Health Status States
    //!
    //! - **Healthy**: Resource is fully operational
    //! - **Degraded**: Resource is slow but functional (high latency)
    //! - **Unhealthy**: Resource should not be used
    //! - **Unknown**: Not yet checked or check failed
    //!
    //! ## Trade-offs
    //!
    //! ### Advantages
    //! - **Proactive**: Catches issues before use
    //! - **Automatic failover**: No manual intervention needed
    //! - **Flexible selection**: Multiple strategies for different use cases
    //! - **Observable**: Export health status for monitoring
    //!
    //! ### Limitations
    //! - **Not a layer**: Cannot compose with Tower middleware
    //! - **Resource overhead**: Background health checks consume resources
    //! - **Complexity**: Requires managing multiple resource instances
    //! - **Health check design**: Poor health checks give false positives/negatives
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Database Failover
    //! ├─ Primary database: healthy
    //! ├─ Secondary database: healthy
    //! ├─ Primary fails → automatic switch to secondary
    //! └─ Primary recovers → can switch back
    //!
    //! Regional API Endpoints
    //! ├─ us-west: healthy (50ms latency)
    //! ├─ us-east: healthy (120ms latency)
    //! ├─ eu-west: degraded (300ms latency)
    //! └─ Round-robin between us-west and us-east (eu-west used only if needed)
    //!
    //! Redis Cluster
    //! ├─ Node 1: healthy
    //! ├─ Node 2: healthy
    //! ├─ Node 3: unhealthy (connection refused)
    //! └─ Distribute load across nodes 1 and 2
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Too frequent checks**: Health checks every 100ms waste resources
    //! ✅ Check every 5-30 seconds for most use cases
    //!
    //! ❌ **Expensive health checks**: Full database query takes 2 seconds
    //! ✅ Simple ping/SELECT 1 takes milliseconds
    //!
    //! ❌ **No threshold**: One failure marks as unhealthy
    //! ✅ Require 2-3 consecutive failures to prevent flapping
    //!
    //! ❌ **Ignoring degraded state**: Treat slow as failed
    //! ✅ Use degraded resources when all healthy ones are down
    //!
    //! ## Example
    //!
    //! ```rust,ignore
    //! use tower_resilience_healthcheck::{
    //!     HealthCheckWrapper, HealthStatus, SelectionStrategy
    //! };
    //! use std::time::Duration;
    //!
    //! # #[derive(Clone)]
    //! # struct Database { name: String }
    //! # impl Database {
    //! #     async fn ping(&self) -> Result<(), std::io::Error> { Ok(()) }
    //! # }
    //! # async fn example() {
    //! # let primary_db = Database { name: "primary".into() };
    //! # let secondary_db = Database { name: "secondary".into() };
    //! // Create wrapper with multiple databases
    //! let wrapper = HealthCheckWrapper::builder()
    //!     .with_context(primary_db, "primary")
    //!     .with_context(secondary_db, "secondary")
    //!     .with_checker(|db| async move {
    //!         match db.ping().await {
    //!             Ok(_) => HealthStatus::Healthy,
    //!             Err(_) => HealthStatus::Unhealthy,
    //!         }
    //!     })
    //!     .with_interval(Duration::from_secs(10))
    //!     .with_failure_threshold(3)  // 3 failures before marking unhealthy
    //!     .with_success_threshold(2)  // 2 successes to recover
    //!     .with_selection_strategy(SelectionStrategy::RoundRobin)
    //!     .build();
    //!
    //! // Start background health checking
    //! wrapper.start().await;
    //!
    //! // Get a healthy database
    //! if let Some(db) = wrapper.get_healthy().await {
    //!     // Use healthy database
    //! }
    //!
    //! // Get health status for monitoring
    //! let details = wrapper.get_health_details().await;
    //! for detail in details {
    //!     println!("{}: {:?}", detail.name, detail.status);
    //! }
    //! # }
    //! ```
}

/// Reconnect pattern guide
pub mod reconnect {
    //! # Reconnect
    //!
    //! Automatically reconnects to services with configurable backoff strategies when
    //! connection failures occur. Designed for **persistent connections** where the connection
    //! state matters (databases, Redis, message queues, WebSockets).
    //!
    //! ## Reconnect vs Retry
    //!
    //! **Key distinction**: Reconnect manages **connection lifecycle**, Retry manages **operation resilience**.
    //!
    //! - **Reconnect**: Use for persistent connections that can break (Redis, databases, gRPC streams)
    //! - **Retry**: Use for transient request failures on working connections (timeouts, rate limits)
    //!
    //! For persistent connection services, you often want BOTH:
    //! - Reconnect layer handles connection-level errors (BrokenPipe, ConnectionReset)
    //! - Retry layer handles application-level errors (RateLimited, Busy, Timeout)
    //!
    //! ## When to Use
    //!
    //! - **Persistent connections**: Redis, databases, message queues, WebSockets
    //! - **Unstable connections**: Network issues, transient failures
    //! - **Service restarts**: Backend services that periodically restart
    //! - **Connection pooling**: Reconnect stale or broken connections
    //! - **Distributed systems**: Handle network partitions gracefully
    //!
    //! ## Trade-offs
    //!
    //! - **Latency impact**: Reconnection attempts add delay to requests
    //! - **Resource usage**: Failed connections consume resources during backoff
    //! - **Complexity**: Adds state management for connection tracking
    //! - **Thundering herd**: Multiple clients reconnecting simultaneously
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Database Connection Pool
    //! ├─ Connection closed by server after idle timeout
    //! ├─ Reconnect with exponential backoff (100ms -> 5s)
    //! ├─ Retry original query after successful reconnection
    //! └─ Application remains resilient to connection drops
    //!
    //! Message Queue Consumer
    //! ├─ Broker temporarily unavailable during deployment
    //! ├─ Reconnect with fixed 1s intervals, unlimited attempts
    //! ├─ Resume consuming messages when broker returns
    //! └─ No message loss or manual intervention
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Immediate retry**: Overwhelming failing service
    //! ✅ Use exponential backoff to give service time to recover
    //!
    //! ❌ **Unlimited attempts without monitoring**: Silent failures pile up
    //! ✅ Set max attempts for user-facing operations, monitor reconnection rates
    //!
    //! ❌ **No connection state tracking**: Can't determine system health
    //! ✅ Expose connection state for health checks and observability
    //!
    //! ❌ **Reconnecting on non-retryable errors**: Permanent failures waste resources
    //! ✅ Distinguish transient (network) from permanent (auth) errors
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "reconnect")]
    //! # {
    //! use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig, ReconnectPolicy};
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let database_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let reconnect = ReconnectLayer::new(
    //!     ReconnectConfig::builder()
    //!         .policy(ReconnectPolicy::exponential(
    //!             Duration::from_millis(100),  // Start at 100ms
    //!             Duration::from_secs(5),      // Max 5 seconds
    //!         ))
    //!         .max_attempts(10)
    //!         .retry_on_reconnect(true)  // Retry original request
    //!         .build()
    //! );
    //!
    //! let service = reconnect.layer(database_service);
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
    //!     .max_wait_duration(Duration::from_secs(5))
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
    //! let time_limiter = TimeLimiterLayer::<()>::builder()
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
    //! let retry = RetryLayer::<(), MyError>::builder()
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

/// Fallback pattern guide
pub mod fallback {
    //! # Fallback
    //!
    //! Provides alternative responses when services fail, ensuring graceful degradation
    //! instead of error propagation.
    //!
    //! ## Fallback vs Circuit Breaker Fallback
    //!
    //! **Key distinction**: Standalone Fallback is **composable**, Circuit Breaker fallback is **integrated**.
    //!
    //! - **Standalone Fallback**: Works with any layer, flexible strategies, selective error handling
    //! - **Circuit Breaker `.with_fallback()`**: Only triggers when circuit is open
    //!
    //! Use standalone Fallback when you want fallback behavior independent of circuit state,
    //! or when composing with layers other than circuit breaker.
    //!
    //! ## Fallback Strategies
    //!
    //! ### Value
    //! Return a static fallback value. Best for simple default responses.
    //!
    //! ### FromError
    //! Compute fallback from the error. Best when fallback depends on error type.
    //!
    //! ### FromRequestError
    //! Compute fallback from both request and error. Best for request-specific defaults.
    //!
    //! ### Service
    //! Delegate to a fallback service. Best for complex fallback logic or secondary backends.
    //!
    //! ### Exception
    //! Transform the error instead of providing a response. Best for error normalization.
    //!
    //! ## When to Use
    //!
    //! - **Graceful degradation**: Show cached/default content when live data unavailable
    //! - **User experience**: Never show raw errors to users
    //! - **Partial failures**: Some data is better than no data
    //! - **Secondary backends**: Fall back to backup service
    //! - **Default values**: Return sensible defaults for missing data
    //!
    //! ## Trade-offs
    //!
    //! - **Data freshness**: Fallback data may be stale or incomplete
    //! - **Silent failures**: Errors may be hidden from monitoring
    //! - **Complexity**: Multiple code paths to maintain
    //! - **Testing**: Need to verify fallback behavior works correctly
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Product Catalog API
    //! ├─ Primary: Live inventory service
    //! ├─ Fallback: Cached catalog (possibly stale)
    //! ├─ User sees products even during outage
    //! └─ "Inventory may be outdated" warning shown
    //!
    //! User Profile Service
    //! ├─ Primary: Database query
    //! ├─ Fallback: Default avatar and "Guest" name
    //! ├─ Page renders even if profile service down
    //! └─ Graceful degradation vs error page
    //!
    //! Search Service
    //! ├─ Primary: Elasticsearch cluster
    //! ├─ Fallback: Simple database LIKE query
    //! ├─ Slower but functional search
    //! └─ Better than "Search unavailable"
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Hiding all errors**: Critical failures go unnoticed
    //! ✅ Use predicates to only handle expected failures, log/alert on others
    //!
    //! ❌ **Stale fallback data**: Users see outdated information
    //! ✅ Show indicators when using fallback data, set reasonable cache TTLs
    //!
    //! ❌ **Fallback that can also fail**: Cascading fallback failures
    //! ✅ Make fallback as simple and reliable as possible
    //!
    //! ❌ **No monitoring**: Can't tell when fallback is being used
    //! ✅ Use event listeners to track fallback usage and alert on high rates
    //!
    //! ## Example: Static Value Fallback
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "fallback")]
    //! # {
    //! use tower_resilience::fallback::FallbackLayer;
    //! use tower::Layer;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct ApiError;
    //! # async fn example() {
    //! # let api_client = tower::service_fn(|_req: String| async { Err::<String, _>(ApiError) });
    //! let fallback = FallbackLayer::<String, String, ApiError>::value(
    //!     "Service temporarily unavailable".to_string()
    //! );
    //!
    //! let service = fallback.layer(api_client);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Dynamic Fallback from Error
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "fallback")]
    //! # {
    //! use tower_resilience::fallback::FallbackLayer;
    //! use tower::Layer;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct ApiError { code: u16, message: String }
    //! # async fn example() {
    //! # let api_client = tower::service_fn(|_req: String| async {
    //! #     Err::<String, _>(ApiError { code: 503, message: "down".into() })
    //! # });
    //! let fallback = FallbackLayer::<String, String, ApiError>::from_error(|e| {
    //!     format!("Error {}: {}", e.code, e.message)
    //! });
    //!
    //! let service = fallback.layer(api_client);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Selective Fallback with Predicate
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "fallback")]
    //! # {
    //! use tower_resilience::fallback::FallbackLayer;
    //! use tower::Layer;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct ApiError { code: u16 }
    //! # async fn example() {
    //! # let api_client = tower::service_fn(|_req: String| async {
    //! #     Err::<String, _>(ApiError { code: 503 })
    //! # });
    //! // Only provide fallback for 5xx errors, propagate 4xx
    //! let fallback: FallbackLayer<String, String, ApiError> = FallbackLayer::builder()
    //!     .value("Server error fallback".to_string())
    //!     .handle(|e: &ApiError| e.code >= 500)
    //!     .build();
    //!
    //! let service = fallback.layer(api_client);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Service-Based Fallback
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "fallback")]
    //! # {
    //! use tower_resilience::fallback::FallbackLayer;
    //! use tower::Layer;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct ApiError;
    //! # async fn example() {
    //! # let primary_service = tower::service_fn(|_req: String| async { Err::<String, _>(ApiError) });
    //! // Fallback to a backup service
    //! let fallback = FallbackLayer::<String, String, ApiError>::service(|req| {
    //!     Box::pin(async move {
    //!         // Call backup service, return cached data, etc.
    //!         Ok(format!("Backup response for: {}", req))
    //!     })
    //! });
    //!
    //! let service = fallback.layer(primary_service);
    //! # }
    //! # }
    //! ```
}

/// Executor pattern guide
pub mod executor {
    //! # Executor
    //!
    //! Delegates request processing to dedicated executors for parallel execution,
    //! runtime isolation, or thread pool delegation.
    //!
    //! ## When to Use
    //!
    //! - **CPU-bound processing**: Parallelize CPU-intensive request handling
    //! - **Runtime isolation**: Process requests on a dedicated runtime
    //! - **Thread pool delegation**: Use specific thread pools for certain workloads
    //! - **Work stealing**: Distribute work across multiple worker threads
    //!
    //! ## Trade-offs
    //!
    //! - **Overhead**: Spawning tasks adds scheduling overhead
    //! - **Context switching**: Work on different runtimes incurs switching costs
    //! - **Complexity**: Managing multiple runtimes adds operational complexity
    //! - **Resource usage**: Additional runtimes consume memory and threads
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! CPU-Heavy Image Processing
    //! ├─ Main runtime handles HTTP requests
    //! ├─ Executor delegates to compute runtime (8 workers)
    //! ├─ Image processing runs in parallel
    //! └─ Main runtime stays responsive for other requests
    //!
    //! Mixed Workload API
    //! ├─ I/O-bound endpoints: default runtime
    //! ├─ CPU-bound endpoints: compute runtime
    //! ├─ Background jobs: background runtime
    //! └─ Each workload gets appropriate resources
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Executor for simple I/O**: Overhead exceeds benefit
    //! ✅ Only use executor for CPU-bound or isolation requirements
    //!
    //! ❌ **Too many runtimes**: Resource fragmentation
    //! ✅ Use 2-3 runtimes max, sized appropriately
    //!
    //! ❌ **Blocking in executor**: Starves worker threads
    //! ✅ Use spawn_blocking for blocking operations
    //!
    //! ## Example
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "executor")]
    //! # {
    //! use tower_resilience::executor::ExecutorLayer;
    //! use tower::ServiceBuilder;
    //!
    //! # async fn example() {
    //! # let my_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! // Create a dedicated compute runtime
    //! let compute_runtime = tokio::runtime::Builder::new_multi_thread()
    //!     .worker_threads(8)
    //!     .thread_name("compute")
    //!     .build()
    //!     .unwrap();
    //!
    //! let layer = ExecutorLayer::new(compute_runtime.handle().clone());
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(my_service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Current Runtime
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "executor")]
    //! # {
    //! use tower_resilience::executor::ExecutorLayer;
    //! use tower::ServiceBuilder;
    //!
    //! # async fn example() {
    //! # let my_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! // Use the current tokio runtime
    //! let layer = ExecutorLayer::current();
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(my_service);
    //! # }
    //! # }
    //! ```
}

/// Hedge pattern guide
pub mod hedge {
    //! # Hedge
    //!
    //! Reduces tail latency by executing parallel redundant requests. When the primary
    //! request is slow, hedging fires additional requests and returns whichever
    //! completes first successfully.
    //!
    //! ## Hedge vs Retry
    //!
    //! **Key distinction**: Hedge runs requests **in parallel**, Retry runs them **sequentially**.
    //!
    //! - **Hedge**: Fire backup requests while primary is still running (latency optimization)
    //! - **Retry**: Wait for failure, then try again (reliability optimization)
    //!
    //! Use Hedge when latency matters more than resource usage. Use Retry for fault tolerance.
    //!
    //! ## Hedging Modes
    //!
    //! ### Latency Mode (delay > 0)
    //! Wait for a specified duration before firing hedge requests. Only fires hedges
    //! if the primary is slow. This is the default and most resource-efficient mode.
    //!
    //! ### Parallel Mode (delay = 0)
    //! Fire all requests simultaneously. Returns the fastest response. Maximum latency
    //! reduction at the cost of higher resource usage.
    //!
    //! ### Dynamic Delay
    //! Adjust delay based on attempt number or other factors. Useful for graduated
    //! hedging strategies.
    //!
    //! ## When to Use
    //!
    //! - **Tail latency critical**: P99/P999 latency matters (trading systems, real-time)
    //! - **Idempotent operations**: Safe to execute multiple times (reads, idempotent writes)
    //! - **Variable backend latency**: Some backends occasionally slow but usually fast
    //! - **Low-cost operations**: Extra requests are cheap relative to latency improvement
    //!
    //! ## When NOT to Use
    //!
    //! ❌ **Non-idempotent operations**: Hedging POST /transfer could transfer money twice
    //! ❌ **Resource-constrained backends**: Extra load could make things worse
    //! ❌ **High-cost operations**: Hedging expensive operations wastes resources
    //! ❌ **Consistently slow backends**: All requests will be slow; hedging won't help
    //!
    //! ## Trade-offs
    //!
    //! - **Latency vs resource usage**: Hedging uses more backend resources
    //! - **Amplification**: N hedges = N times the backend load in worst case
    //! - **Complexity**: Need to handle multiple in-flight requests
    //! - **Cost**: More compute, network, and backend capacity needed
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Database Read Latency
    //! ├─ Primary query starts
    //! ├─ After 50ms, primary still running → fire hedge query
    //! ├─ Hedge completes in 20ms (hit hot cache replica)
    //! ├─ Return hedge result, cancel primary
    //! └─ P99 latency reduced from 200ms to 70ms
    //!
    //! Multi-Region API
    //! ├─ Parallel mode: fire to all 3 regions simultaneously
    //! ├─ us-west responds in 30ms (fastest)
    //! ├─ Return us-west result, ignore slower regions
    //! └─ User always gets fastest available response
    //!
    //! Key-Value Store Lookup
    //! ├─ Primary request to shard A
    //! ├─ After 10ms, fire hedge to replica B
    //! ├─ Primary succeeds at 15ms, hedge cancelled
    //! └─ Normal case: no extra load; slow case: hedging saves latency
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Hedging non-idempotent operations**: Duplicate side effects
    //! ✅ Only hedge reads or idempotent writes with proper deduplication
    //!
    //! ❌ **Too aggressive hedging**: Delay too short, too many hedges
    //! ✅ Set delay to P50-P75 latency, limit max_hedged_attempts (2-3)
    //!
    //! ❌ **Hedging to same backend**: Same slow node handles hedge
    //! ✅ Ensure hedges route to different nodes/replicas
    //!
    //! ❌ **No monitoring**: Can't tell if hedging is helping or hurting
    //! ✅ Track hedge success rate, primary vs hedge wins, resource amplification
    //!
    //! ❌ **Hedging already-optimized endpoints**: Diminishing returns
    //! ✅ Target high-variance latency endpoints where hedging provides value
    //!
    //! ## Example: Latency Mode
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "hedge")]
    //! # {
    //! use tower_resilience::hedge::HedgeLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct DbError;
    //! # impl std::fmt::Display for DbError {
    //! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "err") }
    //! # }
    //! # impl std::error::Error for DbError {}
    //! # async fn example() {
    //! # let database_query = tower::service_fn(|_req: String| async { Ok::<String, DbError>(String::new()) });
    //! // Fire hedge after 50ms if primary hasn't responded
    //! let hedge = HedgeLayer::<String, String, DbError>::builder()
    //!     .name("db-query-hedge")
    //!     .delay(Duration::from_millis(50))
    //!     .max_hedged_attempts(2)
    //!     .build();
    //!
    //! let service = hedge.layer(database_query);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Parallel Mode
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "hedge")]
    //! # {
    //! use tower_resilience::hedge::HedgeLayer;
    //! use tower::Layer;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct ApiError;
    //! # impl std::fmt::Display for ApiError {
    //! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "err") }
    //! # }
    //! # impl std::error::Error for ApiError {}
    //! # async fn example() {
    //! # let multi_region_api = tower::service_fn(|_req: String| async { Ok::<String, ApiError>(String::new()) });
    //! // Fire all 3 requests immediately, return fastest
    //! let hedge = HedgeLayer::<String, String, ApiError>::builder()
    //!     .name("multi-region-hedge")
    //!     .no_delay()  // Parallel mode
    //!     .max_hedged_attempts(3)
    //!     .build();
    //!
    //! let service = hedge.layer(multi_region_api);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Dynamic Delay
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "hedge")]
    //! # {
    //! use tower_resilience::hedge::HedgeLayer;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct CacheError;
    //! # impl std::fmt::Display for CacheError {
    //! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "err") }
    //! # }
    //! # impl std::error::Error for CacheError {}
    //! # async fn example() {
    //! # let cache_lookup = tower::service_fn(|_req: String| async { Ok::<String, CacheError>(String::new()) });
    //! // Increasing delays: 10ms, 40ms, 90ms...
    //! let hedge = HedgeLayer::<String, String, CacheError>::builder()
    //!     .name("cache-hedge")
    //!     .delay_fn(|attempt| Duration::from_millis(10 * (attempt as u64).pow(2)))
    //!     .max_hedged_attempts(3)
    //!     .build();
    //!
    //! let service = hedge.layer(cache_lookup);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: With Event Monitoring
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "hedge")]
    //! # {
    //! use tower_resilience::hedge::{HedgeLayer, HedgeEvent};
    //! use tower_resilience::core::FnListener;
    //! use tower::Layer;
    //! use std::time::Duration;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # impl std::fmt::Display for MyError {
    //! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "err") }
    //! # }
    //! # impl std::error::Error for MyError {}
    //! # async fn example() {
    //! # let service_fn = tower::service_fn(|_req: String| async { Ok::<String, MyError>(String::new()) });
    //! let hedge = HedgeLayer::<String, String, MyError>::builder()
    //!     .name("monitored-hedge")
    //!     .delay(Duration::from_millis(50))
    //!     .max_hedged_attempts(2)
    //!     .on_event(FnListener::new(|e: &HedgeEvent| {
    //!         match e {
    //!             HedgeEvent::HedgeSucceeded { attempt, duration, .. } => {
    //!                 println!("Hedge {} won in {:?}", attempt, duration);
    //!             }
    //!             HedgeEvent::PrimarySucceeded { duration, .. } => {
    //!                 println!("Primary won in {:?}", duration);
    //!             }
    //!             _ => {}
    //!         }
    //!     }))
    //!     .build();
    //!
    //! let service = hedge.layer(service_fn);
    //! # }
    //! # }
    //! ```
}

/// Adaptive Concurrency pattern guide
pub mod adaptive {
    //! # Adaptive Concurrency
    //!
    //! Dynamically adjusts concurrency limits based on observed latency and error rates.
    //! Unlike static concurrency limits (like Bulkhead), adaptive limiters automatically
    //! find the optimal concurrency for your downstream services.
    //!
    //! ## Adaptive vs Bulkhead
    //!
    //! **Key distinction**: Adaptive is **self-tuning**, Bulkhead is **statically configured**.
    //!
    //! - **Adaptive**: Automatically finds optimal concurrency based on feedback
    //! - **Bulkhead**: Fixed limit you configure upfront
    //!
    //! Use Adaptive when you don't know the right limit or when capacity varies.
    //! Use Bulkhead when you have a known, fixed resource pool.
    //!
    //! ## Algorithms
    //!
    //! ### AIMD (Additive Increase Multiplicative Decrease)
    //!
    //! Classic TCP-style congestion control:
    //! - On success with low latency: increase limit by a fixed amount (e.g., +1)
    //! - On failure or high latency: decrease limit by a factor (e.g., halve it)
    //!
    //! Creates a "sawtooth" pattern as it continuously probes for capacity.
    //! Simple, well-understood, works in most scenarios.
    //!
    //! ### Vegas
    //!
    //! More sophisticated algorithm using RTT measurements:
    //! - Estimates queue depth from RTT variations
    //! - Increases limit when queue is small (under-utilized)
    //! - Decreases limit when queue is large (congested)
    //!
    //! More stable than AIMD, avoids sawtooth pattern, better for latency-sensitive
    //! applications.
    //!
    //! ## When to Use
    //!
    //! - **Unknown capacity**: Don't know optimal concurrency for downstream
    //! - **Variable backends**: Capacity changes due to autoscaling, load, etc.
    //! - **Auto-tuning**: Want "set it and forget it" concurrency management
    //! - **Latency optimization**: Need to keep latency low while maximizing throughput
    //!
    //! ## Trade-offs
    //!
    //! - **Warm-up time**: Takes time to find optimal limit
    //! - **Oscillation**: AIMD continuously probes, causing limit fluctuations
    //! - **Shared fate**: All callers share the same limit
    //! - **Algorithm choice**: AIMD vs Vegas requires understanding your workload
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Auto-scaling Backend
    //! ├─ Backend starts with 2 pods (low capacity)
    //! ├─ Adaptive limiter finds limit ~20
    //! ├─ Backend scales to 10 pods
    //! ├─ Latency drops, limiter increases to ~100
    //! └─ Throughput automatically maximized
    //!
    //! Database Connection Pool
    //! ├─ Unknown optimal connection count
    //! ├─ Vegas algorithm monitors query latency
    //! ├─ Limit increases until queue builds up
    //! ├─ Settles at optimal ~50 connections
    //! └─ Adapts as query patterns change
    //!
    //! External API with Variable Rate Limits
    //! ├─ API has undocumented rate limits
    //! ├─ AIMD probes for capacity
    //! ├─ Backs off when 429s are returned
    //! └─ Finds sustainable request rate
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Too aggressive decrease factor**: Limit drops too fast, under-utilizes
    //! ✅ Start with 0.5-0.9 decrease factor
    //!
    //! ❌ **No minimum limit**: Can drop to 0 and never recover
    //! ✅ Always set min_limit >= 1
    //!
    //! ❌ **Latency threshold too low**: Normal variance triggers decreases
    //! ✅ Set threshold to P90-P99 latency, not P50
    //!
    //! ❌ **Using for isolation**: Adaptive shares limit across all callers
    //! ✅ Use Bulkhead for tenant/resource isolation
    //!
    //! ## Example: AIMD Algorithm
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "adaptive")]
    //! # {
    //! use tower_resilience::adaptive::{AdaptiveLimiterLayer, Aimd};
    //! use tower::ServiceBuilder;
    //! use std::time::Duration;
    //!
    //! # async fn example() {
    //! # let my_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let layer = AdaptiveLimiterLayer::new(
    //!     Aimd::builder()
    //!         .initial_limit(10)
    //!         .min_limit(1)
    //!         .max_limit(100)
    //!         .increase_by(1)
    //!         .decrease_factor(0.5)
    //!         .latency_threshold(Duration::from_millis(100))
    //!         .build()
    //! );
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(my_service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Vegas Algorithm
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "adaptive")]
    //! # {
    //! use tower_resilience::adaptive::{AdaptiveLimiterLayer, Vegas};
    //! use tower::ServiceBuilder;
    //!
    //! # async fn example() {
    //! # let my_service = tower::service_fn(|_req: ()| async { Ok::<_, std::io::Error>(()) });
    //! let layer = AdaptiveLimiterLayer::new(
    //!     Vegas::builder()
    //!         .initial_limit(10)
    //!         .min_limit(1)
    //!         .max_limit(100)
    //!         .alpha(3)   // Increase when queue < 3
    //!         .beta(6)    // Decrease when queue > 6
    //!         .build()
    //! );
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(my_service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Composition with Circuit Breaker
    //!
    //! ```rust,ignore
    //! use tower_resilience::adaptive::{AdaptiveLimiterLayer, Aimd};
    //! use tower_resilience::circuitbreaker::CircuitBreakerLayer;
    //! use tower::ServiceBuilder;
    //!
    //! // Circuit breaker catches catastrophic failures
    //! // Adaptive limiter optimizes throughput
    //! let service = ServiceBuilder::new()
    //!     .layer(CircuitBreakerLayer::builder().build())
    //!     .layer(AdaptiveLimiterLayer::new(Aimd::builder().build()))
    //!     .service(my_service);
    //! ```
}

/// Coalesce pattern guide
pub mod coalesce {
    //! # Coalesce (Singleflight)
    //!
    //! Deduplicates concurrent identical requests, ensuring only one request executes
    //! while others wait for its result. All callers receive a clone of the result.
    //! This prevents "cache stampede" or "thundering herd" problems.
    //!
    //! ## Coalesce vs Cache
    //!
    //! **Key distinction**: Coalesce deduplicates **in-flight requests**, Cache stores **completed results**.
    //!
    //! - **Coalesce**: Multiple concurrent requests for same key share ONE execution
    //! - **Cache**: Stores results after completion for future requests
    //!
    //! These patterns **complement each other perfectly**:
    //! - Cache layer stores completed results
    //! - Coalesce layer prevents stampede on cache miss
    //!
    //! ## How It Works
    //!
    //! ```text
    //! Without Coalesce:                With Coalesce:
    //! ─────────────────                ──────────────
    //! Request A ──→ Backend            Request A ──→ Backend
    //! Request B ──→ Backend            Request B ──┐
    //! Request C ──→ Backend            Request C ──┤ Wait for A
    //! (3 backend calls)                Request D ──┘
    //!                                  (1 backend call, 4 responses)
    //! ```
    //!
    //! ## When to Use
    //!
    //! - **Cache refresh protection**: When cached value expires, multiple requests may
    //!   try to refresh simultaneously. Coalescing ensures only one refresh happens.
    //!
    //! - **Expensive computations**: Deduplicate requests for the same expensive operation
    //!   (e.g., report generation, ML inference, complex queries).
    //!
    //! - **Rate-limited APIs**: Reduce calls to external APIs that have rate limits by
    //!   coalescing identical requests within a time window.
    //!
    //! - **Database queries**: Combine identical queries that arrive within a short window
    //!   to reduce database load.
    //!
    //! - **Microservice fanout**: When multiple internal services request the same data,
    //!   coalesce to avoid redundant downstream calls.
    //!
    //! ## Requirements
    //!
    //! - **Key type**: Must implement `Hash + Eq + Clone + Send + Sync`
    //! - **Response type**: Must implement `Clone` (to distribute to all waiters)
    //! - **Error type**: Must implement `Clone` (errors are also distributed)
    //!
    //! ## Trade-offs
    //!
    //! - **Latency coupling**: All waiters blocked on slowest request
    //! - **Error propagation**: One failure affects all waiters
    //! - **Clone overhead**: Response cloned for each waiter
    //! - **Memory**: In-flight tracking consumes memory
    //!
    //! ## Real-World Scenarios
    //!
    //! ```text
    //! Cache Stampede Prevention
    //! ├─ Popular item cache expires
    //! ├─ 1000 requests arrive simultaneously
    //! ├─ Without coalescing: 1000 database queries
    //! ├─ With coalescing: 1 database query, 1000 cloned responses
    //! └─ Database load reduced by 99.9%
    //!
    //! Report Generation
    //! ├─ User A requests monthly report
    //! ├─ User B requests same report while A's is generating
    //! ├─ Only one report generated
    //! └─ Both users receive the same result
    //!
    //! External API Protection
    //! ├─ Multiple services need weather data for same city
    //! ├─ External API has 100 req/min limit
    //! ├─ Coalescing reduces calls from 50/sec to 1/sec
    //! └─ Stay well under rate limit
    //! ```
    //!
    //! ## Anti-Patterns
    //!
    //! ❌ **Coalescing non-idempotent operations**: Side effects may be skipped
    //! ✅ Only coalesce read operations or truly idempotent writes
    //!
    //! ❌ **Large response objects**: Clone overhead exceeds savings
    //! ✅ Return `Arc<Response>` or small response types
    //!
    //! ❌ **Long-running requests**: Waiters blocked for extended time
    //! ✅ Combine with time limiter to bound wait time
    //!
    //! ❌ **Unique request keys**: Every request has different key
    //! ✅ Design keys to maximize coalescing opportunities
    //!
    //! ## Example: Basic Usage
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "coalesce")]
    //! # {
    //! use tower_resilience::coalesce::CoalesceLayer;
    //! use tower::ServiceBuilder;
    //!
    //! # #[derive(Clone, Hash, Eq, PartialEq)]
    //! # struct Request { user_id: u64 }
    //! # #[derive(Clone)]
    //! # struct Response;
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let user_service = tower::service_fn(|_req: Request| async { Ok::<_, MyError>(Response) });
    //! // Coalesce by user_id - concurrent requests for same user share execution
    //! let layer = CoalesceLayer::new(|req: &Request| req.user_id);
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(user_service);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: With Cache Layer
    //!
    //! ```rust,no_run
    //! # #[cfg(all(feature = "coalesce", feature = "cache"))]
    //! # {
    //! use tower_resilience::coalesce::CoalesceLayer;
    //! use tower_resilience::cache::CacheLayer;
    //! use tower::ServiceBuilder;
    //! use std::time::Duration;
    //!
    //! # #[derive(Clone, Hash, Eq, PartialEq)]
    //! # struct Request { id: String }
    //! # #[derive(Clone)]
    //! # struct Response;
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let backend = tower::service_fn(|_req: Request| async { Ok::<_, MyError>(Response) });
    //! // Cache stores results, Coalesce prevents stampede on cache miss
    //! let cache = CacheLayer::builder()
    //!     .max_size(1000)
    //!     .ttl(Duration::from_secs(300))
    //!     .key_extractor(|req: &Request| req.id.clone())
    //!     .build();
    //!
    //! let coalesce = CoalesceLayer::new(|req: &Request| req.id.clone());
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(cache)      // Check cache first
    //!     .layer(coalesce)   // Coalesce cache misses
    //!     .service(backend);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Example: Named Instance
    //!
    //! ```rust,no_run
    //! # #[cfg(feature = "coalesce")]
    //! # {
    //! use tower_resilience::coalesce::CoalesceLayer;
    //! use tower::ServiceBuilder;
    //!
    //! # #[derive(Debug, Clone)]
    //! # struct MyError;
    //! # async fn example() {
    //! # let report_generator = tower::service_fn(|_req: String| async { Ok::<_, MyError>("report".to_string()) });
    //! let layer = CoalesceLayer::builder(|req: &String| req.clone())
    //!     .name("report-coalesce")
    //!     .build();
    //!
    //! let service = ServiceBuilder::new()
    //!     .layer(layer)
    //!     .service(report_generator);
    //! # }
    //! # }
    //! ```
    //!
    //! ## Prior Art
    //!
    //! This pattern is also known as:
    //! - **Singleflight** (Go's `golang.org/x/sync/singleflight`)
    //! - **Request deduplication**
    //! - **Request collapsing**
}
