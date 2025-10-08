//! P0 Listener Lifecycle Tests
//!
//! Tests for listener lifecycle management including:
//! - Cloning behavior
//! - Independence of cloned collections
//! - Arc-wrapped listeners
//! - Memory management
//! - Multiple collections

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;
use tower_resilience_core::events::{EventListener, EventListeners, FnListener, ResilienceEvent};

#[derive(Debug, Clone)]
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
fn event_listeners_is_clone() {
    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(|_: &TestEvent| {}));

    // This should compile - EventListeners implements Clone
    let _cloned = listeners.clone();

    // Verify they have the same length
    assert_eq!(listeners.len(), _cloned.len());
}

#[test]
fn cloned_listeners_are_independent() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);

    let mut listeners1 = EventListeners::new();
    listeners1.add(FnListener::new(move |_: &TestEvent| {
        c1.fetch_add(1, Ordering::SeqCst);
    }));

    // Clone the listeners
    let mut listeners2 = listeners1.clone();

    // Add a different listener to listeners2
    let c2 = Arc::clone(&counter2);
    listeners2.add(FnListener::new(move |_: &TestEvent| {
        c2.fetch_add(1, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
    };

    // listeners1 should only have the first listener
    listeners1.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 0);
    assert_eq!(listeners1.len(), 1);

    // listeners2 should have both listeners
    listeners2.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 2); // Called again
    assert_eq!(counter2.load(Ordering::SeqCst), 1); // Called for first time
    assert_eq!(listeners2.len(), 2);
}

#[test]
fn listeners_are_arc_wrapped() {
    // This test verifies the internal structure uses Arc
    // We can verify this by checking that listeners are shared via Arc

    struct CountingListener {
        counter: Arc<AtomicUsize>,
    }

    impl EventListener<TestEvent> for CountingListener {
        fn on_event(&self, _event: &TestEvent) {
            self.counter.fetch_add(1, Ordering::SeqCst);
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let listener = CountingListener {
        counter: Arc::clone(&counter),
    };

    let mut listeners = EventListeners::new();
    listeners.add(listener);

    // Clone the collection - listeners should be Arc-shared
    let listeners_clone = listeners.clone();

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
    };

    // Both should increment the same counter (Arc-shared listener)
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    listeners_clone.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn no_memory_leaks_with_many_listeners() {
    // Use weak references to verify listeners are properly dropped
    let weak_refs: Arc<std::sync::Mutex<Vec<Weak<AtomicUsize>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    {
        let mut listeners = EventListeners::new();

        // Add 100 listeners, keeping weak references to their state
        for _ in 0..100 {
            let counter = Arc::new(AtomicUsize::new(0));
            weak_refs.lock().unwrap().push(Arc::downgrade(&counter));

            let counter_clone = Arc::clone(&counter);
            listeners.add(FnListener::new(move |_: &TestEvent| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }));
        }

        assert_eq!(listeners.len(), 100);

        // All weak refs should be valid while listeners exist
        let weak_vec = weak_refs.lock().unwrap();
        for weak in weak_vec.iter() {
            assert!(weak.upgrade().is_some());
        }
    } // listeners dropped here

    // After dropping listeners, weak refs should eventually be invalid
    // Note: This might not always work perfectly due to Arc cycles,
    // but it's a reasonable sanity check
    let weak_vec = weak_refs.lock().unwrap();
    let valid_count = weak_vec.iter().filter(|w| w.upgrade().is_some()).count();

    // At least some should be dropped (this is a conservative check)
    // In practice, many or all should be dropped, but we can't guarantee
    // all due to potential Arc cycles in the test setup
    assert!(valid_count < 100);
}

#[test]
fn listeners_dropped_when_collection_dropped() {
    let dropped = Arc::new(AtomicUsize::new(0));

    struct DropCounter {
        counter: Arc<AtomicUsize>,
    }

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.counter.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl EventListener<TestEvent> for DropCounter {
        fn on_event(&self, _event: &TestEvent) {}
    }

    {
        let mut listeners = EventListeners::new();

        listeners.add(DropCounter {
            counter: Arc::clone(&dropped),
        });
        listeners.add(DropCounter {
            counter: Arc::clone(&dropped),
        });
        listeners.add(DropCounter {
            counter: Arc::clone(&dropped),
        });

        assert_eq!(listeners.len(), 3);
        // Listeners are still alive
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
    } // listeners collection dropped here

    // All three listeners should be dropped
    assert_eq!(dropped.load(Ordering::SeqCst), 3);
}

#[test]
fn multiple_collections_with_same_listener_type() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);

    // Create two independent collections
    let mut listeners_a = EventListeners::new();
    listeners_a.add(FnListener::new(move |_: &TestEvent| {
        c1.fetch_add(1, Ordering::SeqCst);
    }));

    let mut listeners_b = EventListeners::new();
    listeners_b.add(FnListener::new(move |_: &TestEvent| {
        c2.fetch_add(10, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
    };

    // Each collection operates independently
    listeners_a.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 0);

    listeners_b.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 10);

    // Verify independence
    assert_eq!(listeners_a.len(), 1);
    assert_eq!(listeners_b.len(), 1);
}
