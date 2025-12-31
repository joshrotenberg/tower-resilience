//! Layer implementation for the executor middleware.

use crate::{Executor, ExecutorService};
use tower_layer::Layer;

/// A Tower layer that delegates request processing to an executor.
///
/// This layer wraps a service and spawns each request's processing
/// as a new task on the provided executor. This enables parallel
/// processing of requests across multiple executor threads.
///
/// # Example
///
/// ```rust,no_run
/// use tower_resilience_executor::ExecutorLayer;
/// use tokio::runtime::Handle;
///
/// // Use the current runtime
/// let layer = ExecutorLayer::new(Handle::current());
/// ```
#[derive(Clone)]
pub struct ExecutorLayer<E> {
    executor: E,
}

impl<E> ExecutorLayer<E>
where
    E: Executor,
{
    /// Creates a new executor layer with the given executor.
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Creates a builder for configuring the executor layer.
    pub fn builder() -> ExecutorLayerBuilder<E> {
        ExecutorLayerBuilder::new()
    }
}

impl ExecutorLayer<tokio::runtime::Handle> {
    /// Creates an executor layer using the current tokio runtime.
    ///
    /// # Panics
    ///
    /// Panics if called from outside a tokio runtime.
    pub fn current() -> Self {
        Self::new(tokio::runtime::Handle::current())
    }
}

impl<S, E> Layer<S> for ExecutorLayer<E>
where
    E: Clone,
{
    type Service = ExecutorService<S, E>;

    fn layer(&self, service: S) -> Self::Service {
        ExecutorService::new(service, self.executor.clone())
    }
}

/// Builder for configuring an [`ExecutorLayer`].
pub struct ExecutorLayerBuilder<E> {
    executor: Option<E>,
}

impl<E> ExecutorLayerBuilder<E> {
    /// Creates a new builder.
    fn new() -> Self {
        Self { executor: None }
    }
}

impl<E> ExecutorLayerBuilder<E>
where
    E: Executor,
{
    /// Sets the executor to use for spawning request processing.
    pub fn executor(mut self, executor: E) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Builds the executor layer.
    ///
    /// # Panics
    ///
    /// Panics if no executor was configured.
    pub fn build(self) -> ExecutorLayer<E> {
        ExecutorLayer {
            executor: self.executor.expect("executor must be configured"),
        }
    }
}

impl ExecutorLayerBuilder<tokio::runtime::Handle> {
    /// Sets a tokio runtime handle as the executor.
    pub fn handle(mut self, handle: tokio::runtime::Handle) -> Self {
        self.executor = Some(handle);
        self
    }

    /// Uses the current tokio runtime as the executor.
    ///
    /// # Panics
    ///
    /// Panics if called from outside a tokio runtime.
    pub fn current(mut self) -> Self {
        self.executor = Some(tokio::runtime::Handle::current());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_layer_creation() {
        let layer = ExecutorLayer::current();
        let _layer2 = layer.clone();
    }

    #[tokio::test]
    async fn test_builder() {
        let layer = ExecutorLayer::<tokio::runtime::Handle>::builder()
            .current()
            .build();
        let _layer2 = layer.clone();
    }

    #[tokio::test]
    async fn test_builder_with_handle() {
        let handle = tokio::runtime::Handle::current();
        let layer = ExecutorLayer::builder().handle(handle).build();
        let _layer2 = layer.clone();
    }
}
