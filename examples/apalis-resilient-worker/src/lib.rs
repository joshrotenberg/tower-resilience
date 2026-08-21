use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use apalis::prelude::*;
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, ConcurrencyAlgorithm};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitState};

const DEMO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
struct Job {
    id: usize,
    should_fail: bool,
}

/// Run the finite circuit-breaker, bulkhead, and adaptive-limit scenarios.
pub async fn run_all() -> Result<(), BoxDynError> {
    circuit_breaker_backpressure().await?;
    bulkhead_isolation().await?;
    adaptive_concurrency().await?;
    Ok(())
}

async fn circuit_breaker_backpressure() -> Result<(), BoxDynError> {
    let calls = Arc::new(AtomicUsize::new(0));
    let opened = Arc::new(AtomicUsize::new(0));
    let half_opened = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));

    let opened_listener = Arc::clone(&opened);
    let half_opened_listener = Arc::clone(&half_opened);
    let closed_listener = Arc::clone(&closed);
    let breaker = CircuitBreakerLayer::builder()
        .name("apalis-downstream")
        .consecutive_failures(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .backpressure()
        .on_state_transition(move |from, to| {
            println!("circuit breaker: {from:?} -> {to:?}");
            match to {
                CircuitState::Open => opened_listener.fetch_add(1, Ordering::SeqCst),
                CircuitState::HalfOpen => half_opened_listener.fetch_add(1, Ordering::SeqCst),
                CircuitState::Closed => closed_listener.fetch_add(1, Ordering::SeqCst),
            };
        })
        .build()?;

    let mut storage = MemoryStorage::new();
    storage
        .push(Job {
            id: 1,
            should_fail: true,
        })
        .await?;
    storage
        .push(Job {
            id: 2,
            should_fail: true,
        })
        .await?;
    storage
        .push(Job {
            id: 3,
            should_fail: false,
        })
        .await?;
    storage
        .push(Job {
            id: 4,
            should_fail: false,
        })
        .await?;

    let handler_calls = Arc::clone(&calls);
    let worker = WorkerBuilder::new("circuit-breaker-worker")
        .backend(storage)
        .layer(breaker)
        .build(move |job: Job, worker: WorkerContext| {
            let calls = Arc::clone(&handler_calls);
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                if job.id == 4 {
                    worker.stop()?;
                }
                if job.should_fail {
                    Err::<(), BoxDynError>(io::Error::other("planned downstream failure").into())
                } else {
                    Ok(())
                }
            }
        });

    run_with_timeout(worker.run()).await?;

    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(opened.load(Ordering::SeqCst), 1);
    assert_eq!(half_opened.load(Ordering::SeqCst), 1);
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    println!("circuit breaker preserved queued work while open");
    Ok(())
}

async fn bulkhead_isolation() -> Result<(), BoxDynError> {
    const MAX_CONCURRENT: usize = 2;
    const JOBS: usize = 8;

    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let bulkhead = BulkheadLayer::builder()
        .name("apalis-job-type")
        .max_concurrent_calls(MAX_CONCURRENT)
        .backpressure()
        .build()?;

    let mut storage = MemoryStorage::new();
    for id in 0..JOBS {
        storage
            .push(Job {
                id,
                should_fail: false,
            })
            .await?;
    }

    let handler_in_flight = Arc::clone(&in_flight);
    let handler_max_seen = Arc::clone(&max_seen);
    let worker = WorkerBuilder::new("bulkhead-worker")
        .backend(storage)
        .map_err(box_layer_error)
        .layer(bulkhead)
        .parallelize(tokio::spawn)
        .build(move |job: Job, worker: WorkerContext| {
            let in_flight = Arc::clone(&handler_in_flight);
            let max_seen = Arc::clone(&handler_max_seen);
            async move {
                let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                if job.id == JOBS - 1 {
                    worker.stop()?;
                }
                Ok::<(), BoxDynError>(())
            }
        });

    run_with_timeout(worker.run()).await?;

    let observed = max_seen.load(Ordering::SeqCst);
    assert_eq!(observed, MAX_CONCURRENT);
    println!("bulkhead capped Apalis at {observed} concurrent jobs");
    Ok(())
}

async fn adaptive_concurrency() -> Result<(), BoxDynError> {
    let algorithm = Arc::new(
        Aimd::builder()
            .initial_limit(2)
            .min_limit(1)
            .max_limit(10)
            .increase_by(1)
            .decrease_factor(0.5)
            .latency_threshold(Duration::from_secs(1))
            .build()?,
    );
    let layer = AdaptiveLimiterLayer::new(SharedAimd(Arc::clone(&algorithm)));

    let mut storage = MemoryStorage::new();
    storage
        .push(Job {
            id: 1,
            should_fail: false,
        })
        .await?;
    storage
        .push(Job {
            id: 2,
            should_fail: false,
        })
        .await?;
    storage
        .push(Job {
            id: 3,
            should_fail: true,
        })
        .await?;

    let worker = WorkerBuilder::new("adaptive-worker")
        .backend(storage)
        .map_err(box_layer_error)
        .layer(layer)
        .build(|job: Job, worker: WorkerContext| async move {
            if job.id == 3 {
                worker.stop()?;
            }
            if job.should_fail {
                Err::<(), BoxDynError>(io::Error::other("planned congestion signal").into())
            } else {
                Ok(())
            }
        });

    run_with_timeout(worker.run()).await?;

    // Two fast successes raise the limit from 2 to 4; the failure halves it.
    assert_eq!(algorithm.limit(), 2);
    println!("adaptive limit reacted to Apalis job outcomes: 2 -> 4 -> 2");
    Ok(())
}

async fn run_with_timeout(
    worker: impl Future<Output = Result<(), WorkerError>>,
) -> Result<(), BoxDynError> {
    tokio::time::timeout(DEMO_TIMEOUT, worker)
        .await
        .map_err(|_| io::Error::other("worker demo timed out"))??;
    Ok(())
}

// Apalis task functions expose BoxDynError as their service error. Until #443
// is fixed, generic tower-resilience wrappers around that error need an outer
// adapter to satisfy Apalis's Into<BoxDynError> worker bound.
fn box_layer_error(error: impl std::fmt::Display) -> BoxDynError {
    io::Error::other(error.to_string()).into()
}

#[derive(Clone)]
struct SharedAimd(Arc<Aimd>);

impl ConcurrencyAlgorithm for SharedAimd {
    fn record_success(&self, latency: Duration) {
        self.0.record_success(latency);
    }

    fn record_failure(&self) {
        self.0.record_failure();
    }

    fn record_dropped(&self) {
        self.0.record_dropped();
    }

    fn limit(&self) -> usize {
        self.0.limit()
    }

    fn min_limit(&self) -> usize {
        self.0.min_limit()
    }

    fn max_limit(&self) -> usize {
        self.0.max_limit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn layers_compose_with_apalis_workers() {
        circuit_breaker_backpressure().await.unwrap();
        bulkhead_isolation().await.unwrap();
        adaptive_concurrency().await.unwrap();
    }
}
