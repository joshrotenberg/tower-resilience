//! P1 Panic Handling Tests
//!
//! Tests for panic handling in event listeners including:
//! - Single listener panics
//! - Panic doesn't prevent other listeners
//! - Multiple panicking listeners
//! - Emit returns normally after panic
//! - Panics with complex event types
//!
//! IMPORTANT: The EventListeners::emit() method uses std::panic::catch_unwind
//! to ensure that a panic in one listener doesn't prevent other listeners
//! from being called. This is a resilience feature - the event system itself
//! must be resilient!

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tower_resilience_core::events::{EventListeners, FnListener, ResilienceEvent};

#[derive(Debug, Clone)]
struct TestEvent {
    name: String,
    timestamp: Instant,
    #[allow(dead_code)]
    value: i64,
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
fn listener_that_panics_does_not_crash_emit() {
    let mut listeners = EventListeners::new();

    // Add a listener that panics
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("Intentional panic in listener");
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 42,
    };

    // This should not panic - the panic should be caught
    listeners.emit(&event);

    // If we get here, the panic was successfully caught
    // Test passed - emit returned normally after panic
}

#[test]
fn panic_in_one_listener_does_not_prevent_others() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);
    let c3 = Arc::clone(&counter3);

    let mut listeners = EventListeners::new();

    // First listener - works fine
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c1.fetch_add(1, Ordering::SeqCst);
    }));

    // Second listener - panics
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("Listener 2 panics");
    }));

    // Third listener - should still be called despite listener 2 panicking
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c2.fetch_add(1, Ordering::SeqCst);
    }));

    // Fourth listener - also should be called
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c3.fetch_add(1, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    listeners.emit(&event);

    // All non-panicking listeners should have been called
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 1);
    assert_eq!(counter3.load(Ordering::SeqCst), 1);
}

#[test]
fn multiple_panicking_listeners() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);

    let mut listeners = EventListeners::new();

    // Listener 1 - panics
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("First panic");
    }));

    // Listener 2 - works
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c1.fetch_add(1, Ordering::SeqCst);
    }));

    // Listener 3 - panics
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("Second panic");
    }));

    // Listener 4 - works
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c2.fetch_add(1, Ordering::SeqCst);
    }));

    // Listener 5 - panics
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("Third panic");
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    listeners.emit(&event);

    // Non-panicking listeners should have been called
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 1);
}

#[test]
fn emit_returns_normally_after_panic() {
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    // Add a panicking listener
    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("This listener panics");
    }));

    // Add a normal listener
    listeners.add(FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    // First emit
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Second emit - should work the same way
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 2);

    // Third emit - verify it continues to work
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn panic_with_complex_event_types() {
    #[derive(Debug)]
    struct ComplexEvent {
        name: String,
        timestamp: Instant,
        data: Vec<String>,
        #[allow(dead_code)]
        nested: Option<Box<ComplexEvent>>,
    }

    impl ResilienceEvent for ComplexEvent {
        fn event_type(&self) -> &'static str {
            "complex"
        }

        fn timestamp(&self) -> Instant {
            self.timestamp
        }

        fn pattern_name(&self) -> &str {
            &self.name
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let data_received = Arc::new(std::sync::Mutex::new(Vec::new()));

    let c = Arc::clone(&counter);
    let data_clone = Arc::clone(&data_received);

    let mut listeners = EventListeners::new();

    // Panicking listener
    listeners.add(FnListener::new(|_: &ComplexEvent| {
        panic!("Complex event panic");
    }));

    // Normal listener that processes complex data
    listeners.add(FnListener::new(move |event: &ComplexEvent| {
        c.fetch_add(1, Ordering::SeqCst);
        data_clone.lock().unwrap().extend(event.data.clone());
    }));

    let event = ComplexEvent {
        name: "complex-test".to_string(),
        timestamp: Instant::now(),
        data: vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string(),
        ],
        nested: None,
    };

    listeners.emit(&event);

    assert_eq!(counter.load(Ordering::SeqCst), 1);
    let data = data_received.lock().unwrap();
    assert_eq!(
        *data,
        vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string()
        ]
    );
}

#[test]
fn panic_behavior_documented_in_test() {
    // This test serves as documentation for the panic handling behavior.
    //
    // BEHAVIOR:
    // - EventListeners::emit() uses std::panic::catch_unwind to catch panics
    // - If a listener panics, the panic is caught and execution continues
    // - Other listeners will still be called
    // - emit() returns normally even if listeners panic
    //
    // RATIONALE:
    // - The event system is part of the resilience infrastructure
    // - It must be resilient itself - one bad listener shouldn't break the system
    // - This allows users to add listeners without worrying about breaking others
    // - Particularly important for observability - metrics/logging failures
    //   shouldn't prevent the application from functioning

    let good_listener_called = Arc::new(AtomicUsize::new(0));
    let good_clone = Arc::clone(&good_listener_called);

    let mut listeners = EventListeners::new();

    listeners.add(FnListener::new(|_: &TestEvent| {
        panic!("Bad listener");
    }));

    listeners.add(FnListener::new(move |_: &TestEvent| {
        good_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "documentation-test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    // emit() completes normally despite panic
    listeners.emit(&event);

    // Good listener was called
    assert_eq!(good_listener_called.load(Ordering::SeqCst), 1);

    // Can continue to use the listeners
    listeners.emit(&event);
    assert_eq!(good_listener_called.load(Ordering::SeqCst), 2);
}
