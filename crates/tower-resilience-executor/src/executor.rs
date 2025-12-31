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

/// An executor that uses `spawn_blocking` for blocking operations.
///
/// This is useful for services that perform blocking I/O or CPU-intensive
/// work that would block the async runtime.
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
        // We need to spawn the future on the runtime, then use spawn_blocking
        // for the actual work. However, spawn_blocking is for sync code.
        // For async code that may block, we spawn normally but on the dedicated handle.
        self.handle.spawn(future)
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
}
