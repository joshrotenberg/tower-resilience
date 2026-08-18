//! Executor trait for spawning futures.

use std::future::Future;
use tokio::task::JoinHandle;

/// Trait for executors that can spawn futures.
///
/// This trait abstracts over different execution strategies, allowing
/// services to be run on dedicated runtimes, thread pools, or with
/// different spawning strategies.
///
/// # Example
///
/// ```rust,no_run
/// use tower_resilience_executor::Executor;
/// use tokio::runtime::Handle;
///
/// // Tokio Handle implements Executor
/// let handle = Handle::current();
/// ```
pub trait Executor: Clone + Send + Sync + 'static {
    /// Spawns a future onto this executor.
    ///
    /// Returns a handle that can be used to await the result.
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static;
}

/// Executor implementation for tokio's runtime Handle.
///
/// This spawns futures as new tasks on the tokio runtime.
impl Executor for tokio::runtime::Handle {
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::runtime::Handle::spawn(self, future)
    }
}

/// An executor that runs futures on Tokio's `spawn_blocking` thread pool.
///
/// Unlike [`CurrentRuntime`] and `Handle`, which schedule futures on the
/// runtime's async core worker threads, `BlockingExecutor` reserves a
/// thread from the runtime's dedicated blocking-task pool (bounded, 512
/// threads by default) and drives the future to completion there via
/// [`Handle::block_on`]. This gives services that perform blocking I/O or
/// CPU-intensive work a real thread-class boundary: that work cannot stall
/// the async core workers that the rest of the application depends on,
/// because it never runs on them.
///
/// Use this when a wrapped service's future may call blocking APIs
/// directly (rather than already using `spawn_blocking` internally) --
/// for example, synchronous file I/O, a blocking database driver, or a
/// CPU-bound computation without yield points.
///
/// # Cancellation
///
/// The returned `JoinHandle` supports `.abort()`, but blocking-pool work
/// cannot be preempted once it starts running: if the future has already
/// begun executing on its blocking-pool thread, `abort()` does not stop
/// it -- the future runs to completion (consuming the thread for that
/// duration) and only then does the handle resolve as cancelled. Abort
/// only prevents the closure from starting if it has not yet been
/// scheduled.
///
/// # Backpressure and dispatch
///
/// Each call to [`Executor::spawn`] draws one thread from Tokio's blocking
/// pool. The pool is bounded (`max_blocking_threads`, 512 by default): once
/// exhausted, further work queues for a thread to become available rather
/// than spawning unbounded OS threads. `BlockingExecutor` itself applies no
/// additional admission control -- pair it with a
/// [bulkhead](https://docs.rs/tower-resilience-bulkhead) layer upstream if
/// you need to cap in-flight requests below the pool's capacity.
///
/// [`Handle::block_on`]: tokio::runtime::Handle::block_on
///
/// # Example
///
/// ```rust,no_run
/// use tower_resilience_executor::BlockingExecutor;
/// use tokio::runtime::Handle;
///
/// let executor = BlockingExecutor::new(Handle::current());
/// ```
#[derive(Clone)]
pub struct BlockingExecutor {
    handle: tokio::runtime::Handle,
}

impl BlockingExecutor {
    /// Creates a new blocking executor using the given runtime handle.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }

    /// Creates a new blocking executor using the current runtime handle.
    ///
    /// # Panics
    ///
    /// Panics if called from outside a tokio runtime.
    pub fn current() -> Self {
        Self::new(tokio::runtime::Handle::current())
    }
}

impl Executor for BlockingExecutor {
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        // Reserve a thread from the blocking pool and drive the future to
        // completion there via `Handle::block_on`. This is Tokio's
        // documented bridge pattern for running a future off the async
        // core workers: timers, IO, and wakers still route through the
        // owning runtime, but the future's own polling -- including any
        // blocking calls it makes -- happens on the dedicated thread.
        let handle = self.handle.clone();
        tokio::task::spawn_blocking(move || handle.block_on(future))
    }
}

/// An executor wrapper that spawns on the current runtime.
///
/// This is a convenience type that captures the current runtime handle
/// at construction time.
#[derive(Clone)]
pub struct CurrentRuntime {
    handle: tokio::runtime::Handle,
}

impl CurrentRuntime {
    /// Creates a new executor using the current runtime handle.
    ///
    /// # Panics
    ///
    /// Panics if called from outside a tokio runtime.
    pub fn new() -> Self {
        Self {
            handle: tokio::runtime::Handle::current(),
        }
    }
}

impl Default for CurrentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor for CurrentRuntime {
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_executor() {
        let handle = tokio::runtime::Handle::current();
        let join = handle.spawn(async { 42 });
        assert_eq!(join.await.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_current_runtime_executor() {
        let executor = CurrentRuntime::new();
        let join = executor.spawn(async { 42 });
        assert_eq!(join.await.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_blocking_executor() {
        let executor = BlockingExecutor::current();
        let join = executor.spawn(async { 42 });
        assert_eq!(join.await.unwrap(), 42);
    }

    /// Proves `BlockingExecutor` runs work on a thread separate from the
    /// runtime's async core workers, not merely on the same worker via an
    /// ordinary `Handle::spawn`.
    ///
    /// With a single-worker-thread runtime, blocking that one core worker
    /// would delay any other async work scheduled concurrently. This test
    /// submits a task through `BlockingExecutor` that blocks its OS thread
    /// with `std::thread::sleep` (not `tokio::time::sleep`, which would
    /// yield) and asserts that a concurrently-scheduled, quick async task
    /// on the runtime's own worker still completes promptly -- proving the
    /// blocking work ran on a separate (blocking-pool) thread.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_blocking_executor_isolates_runtime_worker() {
        use std::time::{Duration, Instant};

        let executor = BlockingExecutor::current();
        let start = Instant::now();

        // Submit work that blocks its OS thread for a while.
        let blocking_join = executor.spawn(async {
            std::thread::sleep(Duration::from_millis(300));
        });

        // Give the blocking work a moment to start, then confirm the
        // runtime's single core worker is still responsive to ordinary
        // async work scheduled on it directly.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let responsive_elapsed = start.elapsed();
        assert!(
            responsive_elapsed < Duration::from_millis(250),
            "runtime worker appears stalled by blocking-executor work: {:?}",
            responsive_elapsed
        );

        blocking_join.await.unwrap();
    }
}
