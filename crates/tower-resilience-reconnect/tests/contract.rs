//! Tower contract and fresh-instance regressions for `ReconnectService`.

use std::convert::Infallible;
use std::future::{ready, Ready};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tower::{service_fn, Layer, Service, ServiceExt};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

struct Connection {
    id: usize,
    ready: bool,
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            // Readiness is receiver-local and must never be inherited by a clone.
            ready: false,
        }
    }
}

impl Service<String> for Connection {
    type Response = (usize, String);
    type Error = io::Error;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: String) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without receiver-local readiness"
        );
        self.ready = false;

        if self.id == 1 {
            ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "connection 1 is broken",
            )))
        } else {
            ready(Ok((self.id, request)))
        }
    }
}

#[tokio::test]
async fn reconnect_builds_and_readies_a_fresh_service_instance() {
    let next_id = Arc::new(AtomicUsize::new(0));
    let factory_ids = Arc::clone(&next_id);
    let factory = service_fn(move |(): ()| {
        let id = factory_ids.fetch_add(1, Ordering::SeqCst) + 1;
        async move { Ok::<_, Infallible>(Connection { id, ready: false }) }
    });

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(3)
        .build();
    let mut service = ReconnectLayer::new(config).layer(factory);

    let (connection_id, response) = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_owned())
        .await
        .unwrap();

    assert_eq!(connection_id, 2, "the failed instance must be replaced");
    assert_eq!(response, "hello");
    assert_eq!(next_id.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn clones_share_the_replacement_generation() {
    let next_id = Arc::new(AtomicUsize::new(0));
    let factory_ids = Arc::clone(&next_id);
    let factory = service_fn(move |(): ()| {
        let id = factory_ids.fetch_add(1, Ordering::SeqCst) + 1;
        async move { Ok::<_, Infallible>(Connection { id, ready: false }) }
    });

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(3)
        .build();
    let mut first = ReconnectLayer::new(config).layer(factory);
    let mut second = first.clone();

    let (replacement, _) = first
        .ready()
        .await
        .unwrap()
        .call("first".to_owned())
        .await
        .unwrap();
    let (observed, _) = second
        .ready()
        .await
        .unwrap()
        .call("second".to_owned())
        .await
        .unwrap();

    assert_eq!(replacement, 2);
    assert_eq!(observed, replacement);
    assert_eq!(next_id.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_a_retry_does_not_strand_the_shared_connection() {
    let next_id = Arc::new(AtomicUsize::new(0));
    let factory_ids = Arc::clone(&next_id);
    let factory = service_fn(move |(): ()| {
        let id = factory_ids.fetch_add(1, Ordering::SeqCst) + 1;
        async move { Ok::<_, Infallible>(Connection { id, ready: false }) }
    });

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(5)))
        .max_attempts(3)
        .build();
    let mut service = ReconnectLayer::new(config).layer(factory);

    let mut cancelled = Box::pin(service.ready().await.unwrap().call("cancelled".to_owned()));
    assert!(futures::poll!(cancelled.as_mut()).is_pending());
    drop(cancelled);

    tokio::time::sleep(Duration::from_millis(10)).await;
    let (connection_id, response) = service
        .ready()
        .await
        .unwrap()
        .call("next".to_owned())
        .await
        .unwrap();

    assert_eq!(connection_id, 2);
    assert_eq!(response, "next");
}

#[tokio::test]
async fn factory_failures_follow_the_reconnect_policy() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&factory_calls);
    let factory = service_fn(move |(): ()| {
        let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            if attempt < 3 {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "factory unavailable",
                ))
            } else {
                Ok(Connection {
                    id: attempt,
                    ready: false,
                })
            }
        }
    });

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(3)
        .build();
    let mut service = ReconnectLayer::new(config).layer(factory);

    let (connection_id, response) = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_owned())
        .await
        .unwrap();

    assert_eq!(connection_id, 3);
    assert_eq!(response, "hello");
    assert_eq!(factory_calls.load(Ordering::SeqCst), 3);
}
