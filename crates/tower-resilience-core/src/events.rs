//! Event system for resilience patterns.
//!
//! Provides a unified event system that all resilience patterns can use
//! for observability and monitoring.

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
    /// prevent others from receiving events.
    pub fn emit(&self, event: &E) {
        for listener in &self.listeners {
            // Catch panics to ensure one listener doesn't prevent others from being called
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener.on_event(event);
            }));
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
}
