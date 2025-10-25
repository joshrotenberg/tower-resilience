//! # Use Cases
//!
//! Real-world scenarios and recommendations for applying resilience patterns.

/// Database client use cases
pub mod database {
    //! # Database Clients
    //!
    //! ```text
    //! Read Replicas (Multiple Instances)
    //! ├─ Health Check wrapper for automatic failover
    //! ├─ Reconnect for dropped connections
    //! ├─ Circuit breaker per replica
    //! ├─ Retry on connection errors
    //! ├─ Timeout for slow queries
    //! └─ Cache for hot queries
    //!
    //! Write Path
    //! ├─ Health Check for primary/secondary selection
    //! ├─ Reconnect with exponential backoff
    //! ├─ Retry on deadlocks (exponential backoff)
    //! ├─ Circuit breaker for replica lag
    //! ├─ Bulkhead for write capacity
    //! └─ Timeout for lock waits
    //! ```
}

/// Message queue use cases
pub mod message_queue {
    //! # Message Queue Workers
    //!
    //! ```text
    //! Consumer (Multiple Brokers)
    //! ├─ Health Check for broker selection
    //! ├─ Reconnect to broker on disconnect
    //! ├─ Bulkhead per queue/priority
    //! ├─ Retry with exponential backoff
    //! ├─ Circuit breaker for downstream
    //! └─ Timeout for message processing
    //!
    //! Publisher
    //! ├─ Reconnect with unlimited attempts
    //! ├─ Retry on publish failures
    //! ├─ Circuit breaker for broker health
    //! ├─ Rate limit for broker protection
    //! └─ Bulkhead for connection pool
    //! ```
}

/// Microservices use cases
pub mod microservices {
    //! # Microservices
    //!
    //! ```text
    //! Service-to-Service
    //! ├─ Circuit breaker per dependency
    //! ├─ Retry for transient errors
    //! ├─ Timeout for tail latency
    //! └─ Bulkhead for isolation
    //!
    //! API Gateway
    //! ├─ Rate limiter per tenant
    //! ├─ Bulkhead per backend service
    //! ├─ Circuit breaker per route
    //! └─ Cache for popular responses
    //! ```
}

/// Background job use cases
pub mod background_jobs {
    //! # Background Jobs
    //!
    //! ```text
    //! Job Execution
    //! ├─ Retry with exponential backoff + jitter
    //! ├─ Bulkhead per job type/priority
    //! ├─ Circuit breaker to pause failing jobs
    //! └─ Timeout for runaway jobs
    //! ```
}
