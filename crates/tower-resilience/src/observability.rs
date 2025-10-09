//! Observability guide for tower-resilience.
//!
//! This module provides comprehensive guidance on metrics, tracing, and monitoring
//! for all resilience patterns.

/// Metrics documentation
pub mod metrics {
    //! # Metrics Guide
    //!
    //! All resilience patterns support optional Prometheus-compatible metrics via the `metrics` feature.
    //!
    //! ## Enabling Metrics
    //!
    //! ```toml
    //! [dependencies]
    //! tower-resilience = { version = "0.3", features = ["circuitbreaker", "metrics"] }
    //! metrics = "0.24"
    //! metrics-exporter-prometheus = "0.16"
    //! ```
    //!
    //! ## Instance Naming
    //!
    //! **Always name your instances** to distinguish metrics from multiple patterns:
    //!
    //! ```rust,ignore
    //! // Multiple circuit breakers with different names
    //! let user_cb = CircuitBreakerLayer::builder()
    //!     .name("user-service")  // ← Instance name
    //!     .build();
    //!
    //! let payment_cb = CircuitBreakerLayer::builder()
    //!     .name("payment-service")
    //!     .build();
    //! ```
    //!
    //! Metrics will include the instance name as a label:
    //! ```text
    //! circuitbreaker_calls_total{circuitbreaker="user-service",outcome="success"} 150
    //! circuitbreaker_calls_total{circuitbreaker="payment-service",outcome="success"} 89
    //! ```
    //!
    //! ## Available Metrics by Pattern
    //!
    //! ### Circuit Breaker
    //!
    //! - `circuitbreaker_calls_total{circuitbreaker, outcome}` - Total calls (success/failure/rejected)
    //! - `circuitbreaker_transitions_total{circuitbreaker, from, to}` - State transitions
    //! - `circuitbreaker_state{circuitbreaker, state}` - Current state gauge
    //! - `circuitbreaker_slow_calls_total{circuitbreaker}` - Slow call detections
    //! - `circuitbreaker_call_duration_seconds{circuitbreaker}` - Call duration histogram
    //!
    //! ### Bulkhead
    //!
    //! - `bulkhead_calls_permitted_total{bulkhead}` - Calls that acquired permits
    //! - `bulkhead_calls_rejected_total{bulkhead}` - Calls rejected (no permits)
    //! - `bulkhead_calls_finished_total{bulkhead}` - Successfully completed calls
    //! - `bulkhead_calls_failed_total{bulkhead}` - Failed calls
    //! - `bulkhead_concurrent_calls{bulkhead}` - Current concurrency gauge
    //! - `bulkhead_wait_duration_seconds{bulkhead}` - Wait time histogram
    //! - `bulkhead_call_duration_seconds{bulkhead}` - Call duration histogram
    //!
    //! ### Retry
    //!
    //! - `retry_calls_total{retry, result}` - Total retry operations (success/exhausted)
    //! - `retry_attempts_total{retry}` - Individual retry attempts
    //! - `retry_attempts{retry}` - Attempts per call histogram
    //!
    //! ### Rate Limiter
    //!
    //! - `ratelimiter_calls_total{ratelimiter, result}` - Calls (permitted/rejected)
    //! - `ratelimiter_wait_duration_seconds{ratelimiter}` - Permit wait time histogram
    //!
    //! ### Time Limiter
    //!
    //! - `timelimiter_calls_total{timelimiter, result}` - Calls (success/error/timeout)
    //! - `timelimiter_call_duration_seconds{timelimiter}` - Call duration histogram
    //!
    //! ### Cache
    //!
    //! - `cache_requests_total{cache, result}` - Cache requests (hit/miss)
    //! - `cache_evictions_total{cache}` - Cache evictions
    //! - `cache_size{cache}` - Current cache size gauge
    //!
    //! ## Example Prometheus Queries
    //!
    //! ```promql
    //! # Circuit breaker failure rate
    //! rate(circuitbreaker_calls_total{outcome="failure"}[5m])
    //!   /
    //! rate(circuitbreaker_calls_total[5m]) * 100
    //!
    //! # Bulkhead rejection percentage
    //! rate(bulkhead_calls_rejected_total[5m])
    //!   /
    //! (rate(bulkhead_calls_permitted_total[5m]) + rate(bulkhead_calls_rejected_total[5m]))
    //!   * 100
    //!
    //! # Average retry attempts per call
    //! rate(retry_attempts_total[5m]) / rate(retry_calls_total[5m])
    //!
    //! # Cache hit rate
    //! rate(cache_requests_total{result="hit"}[5m])
    //!   /
    //! rate(cache_requests_total[5m]) * 100
    //!
    //! # P95 call latency
    //! histogram_quantile(0.95,
    //!   rate(circuitbreaker_call_duration_seconds_bucket[5m])
    //! )
    //! ```
    //!
    //! ## Alert Examples
    //!
    //! ```yaml
    //! # Circuit breaker opened
    //! - alert: CircuitBreakerOpen
    //!   expr: circuitbreaker_state{state="Open"} == 1
    //!   for: 1m
    //!
    //! # High failure rate
    //! - alert: HighFailureRate
    //!   expr: |
    //!     rate(circuitbreaker_calls_total{outcome="failure"}[5m])
    //!     / rate(circuitbreaker_calls_total[5m]) > 0.1
    //!   for: 5m
    //!
    //! # Bulkhead saturation
    //! - alert: BulkheadSaturated
    //!   expr: |
    //!     rate(bulkhead_calls_rejected_total[5m])
    //!     / (rate(bulkhead_calls_permitted_total[5m])
    //!        + rate(bulkhead_calls_rejected_total[5m])) > 0.5
    //!   for: 5m
    //! ```
}

/// Tracing documentation
pub mod tracing_guide {
    //! # Tracing Guide
    //!
    //! Enable detailed logging with the `tracing` feature:
    //!
    //! ```toml
    //! [dependencies]
    //! tower-resilience = { version = "0.3", features = ["circuitbreaker", "tracing"] }
    //! tracing-subscriber = "0.3"
    //! ```
    //!
    //! Each pattern emits structured logs at key decision points:
    //!
    //! ```text
    //! DEBUG circuitbreaker: Call succeeded within timeout duration_ms=45 circuitbreaker="user-service"
    //! WARN  circuitbreaker: Circuit opened from=Closed to=Open circuitbreaker="payment-service"
    //! INFO  retry: Request succeeded after retries attempts=3 retry="api-client"
    //! DEBUG bulkhead: Permit acquired after waiting wait_ms=12 bulkhead="db-pool"
    //! ```
}

/// Event system documentation
pub mod events {
    //! # Event System Guide
    //!
    //! All patterns provide an event system for custom observability:
    //!
    //! ```rust,ignore
    //! let circuit_breaker = CircuitBreakerLayer::builder()
    //!     .on_state_transition(|from, to| {
    //!         println!("Circuit transitioned from {:?} to {:?}", from, to);
    //!     })
    //!     .on_call_rejected(|| {
    //!         // Custom handling, e.g., send to alerting system
    //!     })
    //!     .build();
    //! ```
    //!
    //! See individual pattern documentation for available event listeners.
}
