//! Service implementation for request coalescing.

use crate::CoalesceConfig;
use hashbrown::HashMap;
use parking_lot::Mutex;
use std::future::Future;
use std::hash::Hash;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::broadcast;
use tower_service::Service;

#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter};

#[cfg(feature = "tracing")]
use tracing::debug;

/// Error type for coalesced requests.
#[derive(Debug)]
pub enum CoalesceError<E> {
    /// The underlying service returned an error.
    Service(E),
    /// The leader request was cancelled and no result is available.
    LeaderCancelled,
    /// Failed to receive the result from the leader.
    RecvError,
}

impl<E: std::fmt::Display> std::fmt::Display for CoalesceError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoalesceError::Service(e) => write!(f, "service error: {}", e),
            CoalesceError::LeaderCancelled => write!(f, "leader request was cancelled"),
            CoalesceError::RecvError => write!(f, "failed to receive result from leader"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CoalesceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CoalesceError::Service(e) => Some(e),
            _ => None,
        }
    }
}

impl<E: Clone> Clone for CoalesceError<E> {
    fn clone(&self) -> Self {
        match self {
            CoalesceError::Service(e) => CoalesceError::Service(e.clone()),
            CoalesceError::LeaderCancelled => CoalesceError::LeaderCancelled,
            CoalesceError::RecvError => CoalesceError::RecvError,
        }
    }
}

/// Shared state for tracking in-flight requests.
struct InFlight<K, Res, E> {
    /// Map from key to broadcast sender for that key's result.
    requests: Mutex<HashMap<K, broadcast::Sender<Result<Res, E>>>>,
}

impl<K, Res, E> InFlight<K, Res, E>
where
    K: Hash + Eq + Clone,
    Res: Clone,
    E: Clone,
{
    fn new() -> Self {
        Self {
            requests: Mutex::new(HashMap::new()),
        }
    }

    /// Try to become the leader for a key. Returns None if we're the leader,
    /// or Some(receiver) if another request is already in flight.
    fn try_join(&self, key: K) -> Option<broadcast::Receiver<Result<Res, E>>> {
        let mut requests = self.requests.lock();
        if let Some(sender) = requests.get(&key) {
            // Another request is in flight, subscribe to its result
            Some(sender.subscribe())
        } else {
            // We're the leader, create a new broadcast channel
            // Use a capacity of 1 since we only send one result
            let (tx, _rx) = broadcast::channel(1);
            requests.insert(key, tx);
            None
        }
    }

    /// Complete a request and notify all waiters.
    fn complete(&self, key: &K, result: Result<Res, E>) {
        let mut requests = self.requests.lock();
        if let Some(sender) = requests.remove(key) {
            // Send result to all waiters (ignore errors if no receivers)
            let _ = sender.send(result);
        }
    }

    /// Remove a key without sending a result (for cancellation).
    fn cancel(&self, key: &K) {
        let mut requests = self.requests.lock();
        requests.remove(key);
    }
}

/// A service that coalesces concurrent identical requests.
///
/// When multiple requests arrive concurrently with the same key, only the
/// first one executes. The others wait for its result and receive a clone.
pub struct CoalesceService<S, K, Req, F>
where
    S: Service<Req>,
{
    inner: S,
    config: Arc<CoalesceConfig<K, F>>,
    in_flight: Arc<InFlight<K, S::Response, S::Error>>,
    _req: PhantomData<Req>,
}

impl<S, K, Req, F> CoalesceService<S, K, Req, F>
where
    S: Service<Req>,
    S::Response: Clone,
    S::Error: Clone,
    K: Hash + Eq + Clone + Send + Sync + 'static,
    F: Fn(&Req) -> K,
{
    /// Create a new coalescing service.
    pub fn new(inner: S, config: Arc<CoalesceConfig<K, F>>) -> Self {
        #[cfg(feature = "metrics")]
        {
            describe_counter!(
                "coalesce_requests_total",
                "Total number of requests processed by the coalesce layer"
            );
        }

        Self {
            inner,
            config,
            in_flight: Arc::new(InFlight::new()),
            _req: PhantomData,
        }
    }
}

impl<S, K, Req, F> Clone for CoalesceService<S, K, Req, F>
where
    S: Service<Req> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
            in_flight: Arc::clone(&self.in_flight),
            _req: PhantomData,
        }
    }
}

impl<S, K, Req, F> Service<Req> for CoalesceService<S, K, Req, F>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Response: Clone + Send + 'static,
    S::Error: Clone + Send + 'static,
    S::Future: Send,
    K: Hash + Eq + Clone + Send + Sync + 'static,
    Req: Send + 'static,
    F: Fn(&Req) -> K + Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = CoalesceError<S::Error>;
    type Future = CoalesceFuture<S, K, Req>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(CoalesceError::Service)
    }

    fn call(&mut self, request: Req) -> Self::Future {
        let key = (self.config.key_extractor)(&request);

        #[cfg(any(feature = "metrics", feature = "tracing"))]
        let name = self.config.name.as_deref().unwrap_or("<unnamed>");

        // Check if there's already an in-flight request for this key
        if let Some(receiver) = self.in_flight.try_join(key.clone()) {
            // Wait for the leader's result
            #[cfg(feature = "metrics")]
            {
                counter!("coalesce_requests_total", "coalesce" => name.to_string(), "role" => "waiter").increment(1);
            }

            #[cfg(feature = "tracing")]
            debug!(coalesce = %name, "Request coalesced as waiter");

            CoalesceFuture::Waiting { receiver }
        } else {
            // We're the leader, execute the request
            #[cfg(feature = "metrics")]
            {
                counter!("coalesce_requests_total", "coalesce" => name.to_string(), "role" => "leader").increment(1);
            }

            #[cfg(feature = "tracing")]
            debug!(coalesce = %name, "Request executing as leader");

            let future = self.inner.call(request);
            let in_flight = Arc::clone(&self.in_flight);

            CoalesceFuture::Leading {
                future: Box::pin(future),
                key: Some(key),
                in_flight,
            }
        }
    }
}

/// Future for coalesced requests.
pub enum CoalesceFuture<S, K, Req>
where
    S: Service<Req>,
    S::Response: Clone,
    S::Error: Clone,
    K: Hash + Eq + Clone,
{
    /// We're the leader, executing the actual request.
    #[doc(hidden)]
    Leading {
        future: Pin<Box<S::Future>>,
        key: Option<K>,
        #[allow(private_interfaces)]
        in_flight: Arc<InFlight<K, S::Response, S::Error>>,
    },
    /// We're waiting for another request's result.
    Waiting {
        receiver: broadcast::Receiver<Result<S::Response, S::Error>>,
    },
}

impl<S, K, Req> Future for CoalesceFuture<S, K, Req>
where
    S: Service<Req>,
    S::Response: Clone,
    S::Error: Clone,
    K: Hash + Eq + Clone,
{
    type Output = Result<S::Response, CoalesceError<S::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We only access the inner fields, not moving anything
        let this = unsafe { self.get_unchecked_mut() };

        match this {
            CoalesceFuture::Leading {
                future,
                key,
                in_flight,
            } => {
                match future.as_mut().poll(cx) {
                    Poll::Ready(result) => {
                        // Notify all waiters
                        if let Some(k) = key.take() {
                            let result_clone = match &result {
                                Ok(res) => Ok(res.clone()),
                                Err(e) => Err(e.clone()),
                            };
                            in_flight.complete(&k, result_clone);
                        }
                        Poll::Ready(result.map_err(CoalesceError::Service))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
            CoalesceFuture::Waiting { receiver } => {
                // Try to receive the result
                match receiver.try_recv() {
                    Ok(result) => Poll::Ready(result.map_err(CoalesceError::Service)),
                    Err(broadcast::error::TryRecvError::Empty) => {
                        // Not ready yet, register for wakeup
                        // We need to poll the receiver properly
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Err(broadcast::error::TryRecvError::Closed) => {
                        // Leader was cancelled or dropped without sending
                        Poll::Ready(Err(CoalesceError::LeaderCancelled))
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        // Missed the message (shouldn't happen with capacity 1)
                        Poll::Ready(Err(CoalesceError::RecvError))
                    }
                }
            }
        }
    }
}

impl<S, K, Req> Drop for CoalesceFuture<S, K, Req>
where
    S: Service<Req>,
    S::Response: Clone,
    S::Error: Clone,
    K: Hash + Eq + Clone,
{
    fn drop(&mut self) {
        // If we're the leader and being dropped without completing,
        // remove ourselves from the in-flight map so waiters get an error
        if let CoalesceFuture::Leading { key, in_flight, .. } = self {
            if let Some(k) = key.take() {
                in_flight.cancel(&k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coalesce_error_display() {
        let err: CoalesceError<std::io::Error> = CoalesceError::LeaderCancelled;
        assert_eq!(err.to_string(), "leader request was cancelled");

        let err: CoalesceError<std::io::Error> = CoalesceError::RecvError;
        assert_eq!(err.to_string(), "failed to receive result from leader");

        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
        let err = CoalesceError::Service(io_err);
        assert!(err.to_string().contains("service error"));
    }

    #[test]
    fn test_in_flight_basic() {
        let in_flight: InFlight<String, String, String> = InFlight::new();

        // First request becomes leader
        assert!(in_flight.try_join("key1".to_string()).is_none());

        // Second request joins
        assert!(in_flight.try_join("key1".to_string()).is_some());

        // Different key becomes leader
        assert!(in_flight.try_join("key2".to_string()).is_none());

        // Complete key1
        in_flight.complete(&"key1".to_string(), Ok("result".to_string()));

        // New request for key1 becomes leader again
        assert!(in_flight.try_join("key1".to_string()).is_none());
    }
}
