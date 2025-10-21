//! Event system for resilience patterns.
//!
//! Provides a unified event system that all resilience patterns can use
//! for observability and monitoring.

#[cfg(feature = "tracing")]
use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

/// Trait for events emitted by resilience patterns.
pub trait ResilienceEvent: Send + Sync + fmt::Debug {
    /// Returns the type of event (e.g., "state_transition", "call_rejected").
    fn event_type(&self) -> &'static str;

    /// Returns when this event occurred.
    fn timestamp(&self) -> Instant;

    /// Returns the name of the pattern instance that emitted this event.
    fn pattern_name(&self) -> &str;
}

/// Trait for listening to resilience events.
pub trait EventListener<E: ResilienceEvent>: Send + Sync {
    /// Called when an event occurs.
    fn on_event(&self, event: &E);
}

/// Type alias for boxed event listeners.
pub type BoxedEventListener<E> = Arc<dyn EventListener<E>>;

/// A collection of event listeners.
#[derive(Clone)]
pub struct EventListeners<E: ResilienceEvent> {
    listeners: Vec<BoxedEventListener<E>>,
}

impl<E: ResilienceEvent> EventListeners<E> {
    /// Creates a new empty event listener collection.
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
        }
    }

    /// Adds a listener to the collection.
    pub fn add<L>(&mut self, listener: L)
    where
        L: EventListener<E> + 'static,
    {
        self.listeners.push(Arc::new(listener));
    }

    /// Emits an event to all registered listeners.
    ///
    /// If a listener panics, the panic is caught and the remaining listeners
    /// will still be called. This ensures one misbehaving listener doesn't
    /// prevent others from receiving events. When the optional `tracing`
    /// feature is enabled, panicking listeners are logged as warnings; with the
    /// `metrics` feature enabled a counter is incremented for observability.
    pub fn emit(&self, event: &E) {
        for (index, listener) in self.listeners.iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener.on_event(event);
            }));

            if let Err(_panic_payload) = result {
                #[cfg(feature = "tracing")]
                log_listener_panic(index, event, _panic_payload.as_ref());

                #[cfg(feature = "metrics")]
                record_listener_panic_metric(event);

                #[cfg(not(feature = "tracing"))]
                let _ = index;

                #[cfg(not(any(feature = "tracing", feature = "metrics")))]
                let _ = _panic_payload;
            }
        }
    }

    /// Returns true if there are no listeners.
    pub fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    /// Returns the number of listeners.
    pub fn len(&self) -> usize {
        self.listeners.len()
    }
}

impl<E: ResilienceEvent> Default for EventListeners<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// A simple function-based event listener.
pub struct FnListener<E, F>
where
    F: Fn(&E) + Send + Sync,
{
    f: F,
    _phantom: std::marker::PhantomData<E>,
}

impl<E, F> FnListener<E, F>
where
    F: Fn(&E) + Send + Sync,
{
    /// Creates a new function-based listener.
    pub fn new(f: F) -> Self {
        Self {
            f,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<E, F> EventListener<E> for FnListener<E, F>
where
    E: ResilienceEvent,
    F: Fn(&E) + Send + Sync,
{
    fn on_event(&self, event: &E) {
        (self.f)(event)
    }
}

#[cfg(feature = "tracing")]
fn log_listener_panic<E: ResilienceEvent>(
    index: usize,
    event: &E,
    panic_payload: &(dyn Any + Send),
) {
    let panic_message = panic_payload
        .downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| panic_payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string());

    tracing::warn!(
        listener_index = index,
        pattern = event.pattern_name(),
        event_type = event.event_type(),
        panic_message = %panic_message,
        "resilience event listener panicked"
    );
}

#[cfg(feature = "metrics")]
fn record_listener_panic_metric<E: ResilienceEvent>(event: &E) {
    let pattern_label = event.pattern_name().to_string();
    let event_type_label = event.event_type().to_string();

    metrics::counter!(
        "resilience_event_listener_panics_total",
        "pattern" => pattern_label,
        "event_type" => event_type_label
    )
    .increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct TestEvent {
        name: String,
        timestamp: Instant,
    }

    impl ResilienceEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test"
        }

        fn timestamp(&self) -> Instant {
            self.timestamp
        }

        fn pattern_name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_event_listeners() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let mut listeners = EventListeners::new();
        listeners.add(FnListener::new(move |_event: &TestEvent| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let event = TestEvent {
            name: "test".to_string(),
            timestamp: Instant::now(),
        };

        listeners.emit(&event);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        listeners.emit(&event);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_multiple_listeners() {
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let c1 = Arc::clone(&counter1);
        let c2 = Arc::clone(&counter2);

        let mut listeners = EventListeners::new();
        listeners.add(FnListener::new(move |_: &TestEvent| {
            c1.fetch_add(1, Ordering::SeqCst);
        }));
        listeners.add(FnListener::new(move |_: &TestEvent| {
            c2.fetch_add(2, Ordering::SeqCst);
        }));

        let event = TestEvent {
            name: "test".to_string(),
            timestamp: Instant::now(),
        };

        listeners.emit(&event);
        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn listener_panics_increment_metrics_and_keep_processing() {
        use metrics::set_global_recorder;
        use metrics_util::debugging::DebugValue;
        use metrics_util::debugging::DebuggingRecorder;
        use std::sync::LazyLock;

        static RECORDER: LazyLock<DebuggingRecorder> = LazyLock::new(DebuggingRecorder::default);
        let _ = set_global_recorder(&*RECORDER);

        let successful = Arc::new(AtomicUsize::new(0));
        let successful_clone = Arc::clone(&successful);

        let mut listeners = EventListeners::new();
        listeners.add(FnListener::new(|_: &TestEvent| panic!("boom")));
        listeners.add(FnListener::new(move |_: &TestEvent| {
            successful_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let event = TestEvent {
            name: "panic-metric-test".to_string(),
            timestamp: Instant::now(),
        };

        listeners.emit(&event);
        assert_eq!(successful.load(Ordering::SeqCst), 1);

        let snapshot = RECORDER.snapshotter().snapshot().into_vec();
        let panic_metric = snapshot.iter().find(|(key, _, _, value)| {
            key.key().name() == "resilience_event_listener_panics_total"
                && matches!(value, DebugValue::Counter(_))
                && key
                    .key()
                    .labels()
                    .any(|label| label.key() == "pattern" && label.value() == "panic-metric-test")
        });

        let (key, _, _, _) = panic_metric.expect("expected listener panic counter");
        assert!(key
            .key()
            .labels()
            .any(|label| label.key() == "pattern" && label.value() == "panic-metric-test"));
        assert!(key
            .key()
            .labels()
            .any(|label| label.key() == "event_type" && label.value() == "test"));
    }

    // Note: Tracing output for panicking listeners is verified manually in integration
    // tests and real-world usage. Unit testing tracing output capture is fragile across
    // different test environments (CI, local, with/without global subscribers), so we
    // rely on the metrics test above to verify panic handling behavior.
    //
    // The tracing implementation in log_listener_panic() is straightforward and logs:
    // - Event pattern name
    // - Event type
    // - Listener index
    // - Panic message
    //
    // This can be verified by running any example with RUST_LOG=warn and triggering
    // a listener panic.
}
