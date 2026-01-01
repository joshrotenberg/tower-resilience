//! Common test utilities for composition stack tests.

use std::future::Future;
use tower::Service;

/// Creates a mock service from a synchronous function.
///
/// This is a convenience wrapper around `tower::service_fn` for simple cases
/// where the handler logic is synchronous.
pub fn mock_service<Req, Res, Err, F>(
    f: F,
) -> impl Service<Req, Response = Res, Error = Err> + Clone
where
    F: Fn(Req) -> Result<Res, Err> + Clone + Send + 'static,
    Req: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
{
    tower::service_fn(move |req| {
        let result = f(req);
        async move { result }
    })
}

/// Creates a mock service from an async function.
pub fn mock_async_service<Req, Res, Err, F, Fut>(
    f: F,
) -> impl Service<Req, Response = Res, Error = Err> + Clone
where
    F: Fn(Req) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<Res, Err>> + Send + 'static,
    Req: Send + 'static,
    Res: Send + 'static,
    Err: Send + 'static,
{
    tower::service_fn(move |req| {
        let f = f.clone();
        async move { f(req).await }
    })
}
