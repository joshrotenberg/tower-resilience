//! P0 FnListener Tests
//!
//! Tests for the FnListener wrapper including:
//! - Closure capturing
//! - State modification
//! - Complex event types
//! - Constructor
//! - Multiple closures
//! - Trait implementation

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tower_resilience_core::events::{EventListener, EventListeners, FnListener, ResilienceEvent};

#[derive(Debug, Clone)]
struct TestEvent {
    name: String,
    timestamp: Instant,
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
fn fn_listener_with_closure_capturing_arc_data() {
    let counter = Arc::new(AtomicUsize::new(0));
    let values = Arc::new(Mutex::new(Vec::new()));

    let counter_clone = Arc::clone(&counter);
    let values_clone = Arc::clone(&values);

    let listener = FnListener::new(move |event: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        values_clone.lock().unwrap().push(event.value);
    });

    let event1 = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 42,
    };

    let event2 = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 100,
    };

    listener.on_event(&event1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert_eq!(*values.lock().unwrap(), vec![42]);

    listener.on_event(&event2);
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    assert_eq!(*values.lock().unwrap(), vec![42, 100]);
}

#[test]
fn fn_listener_modifies_shared_state_via_arc() {
    #[derive(Debug)]
    struct SharedState {
        call_count: AtomicUsize,
        sum: Mutex<i64>,
        pattern_names: Mutex<Vec<String>>,
    }

    let state = Arc::new(SharedState {
        call_count: AtomicUsize::new(0),
        sum: Mutex::new(0),
        pattern_names: Mutex::new(Vec::new()),
    });

    let state_clone = Arc::clone(&state);

    let listener = FnListener::new(move |event: &TestEvent| {
        state_clone.call_count.fetch_add(1, Ordering::SeqCst);
        *state_clone.sum.lock().unwrap() += event.value;
        state_clone
            .pattern_names
            .lock()
            .unwrap()
            .push(event.pattern_name().to_string());
    });

    let event1 = TestEvent {
        name: "cb-1".to_string(),
        timestamp: Instant::now(),
        value: 10,
    };

    let event2 = TestEvent {
        name: "cb-2".to_string(),
        timestamp: Instant::now(),
        value: 25,
    };

    let event3 = TestEvent {
        name: "cb-3".to_string(),
        timestamp: Instant::now(),
        value: 15,
    };

    listener.on_event(&event1);
    listener.on_event(&event2);
    listener.on_event(&event3);

    assert_eq!(state.call_count.load(Ordering::SeqCst), 3);
    assert_eq!(*state.sum.lock().unwrap(), 50);
    assert_eq!(
        *state.pattern_names.lock().unwrap(),
        vec!["cb-1", "cb-2", "cb-3"]
    );
}

#[test]
fn fn_listener_with_complex_event_types() {
    #[derive(Debug)]
    struct ComplexEvent {
        name: String,
        timestamp: Instant,
        nested_data: Vec<(String, i32)>,
        optional: Option<String>,
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

    let received_data = Arc::new(Mutex::new(Vec::new()));
    let received_optional = Arc::new(Mutex::new(None));

    let data_clone = Arc::clone(&received_data);
    let optional_clone = Arc::clone(&received_optional);

    let listener = FnListener::new(move |event: &ComplexEvent| {
        *data_clone.lock().unwrap() = event.nested_data.clone();
        *optional_clone.lock().unwrap() = event.optional.clone();
    });

    let event = ComplexEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        nested_data: vec![
            ("key1".to_string(), 1),
            ("key2".to_string(), 2),
            ("key3".to_string(), 3),
        ],
        optional: Some("present".to_string()),
    };

    listener.on_event(&event);

    let data = received_data.lock().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0], ("key1".to_string(), 1));
    assert_eq!(data[1], ("key2".to_string(), 2));
    assert_eq!(data[2], ("key3".to_string(), 3));

    let optional = received_optional.lock().unwrap();
    assert_eq!(*optional, Some("present".to_string()));
}

#[test]
fn fn_listener_new_constructor_works() {
    let called = Arc::new(AtomicUsize::new(0));
    let called_clone = Arc::clone(&called);

    // Use the new() constructor explicitly
    let listener = FnListener::new(move |_event: &TestEvent| {
        called_clone.fetch_add(1, Ordering::SeqCst);
    });

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    listener.on_event(&event);
    assert_eq!(called.load(Ordering::SeqCst), 1);

    listener.on_event(&event);
    assert_eq!(called.load(Ordering::SeqCst), 2);
}

#[test]
fn multiple_fn_listeners_with_different_closures() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);
    let c3 = Arc::clone(&counter3);

    // Create three different FnListeners
    let listener1 = FnListener::new(move |event: &TestEvent| {
        c1.fetch_add(event.value as usize, Ordering::SeqCst);
    });

    let listener2 = FnListener::new(move |event: &TestEvent| {
        c2.fetch_add((event.value * 2) as usize, Ordering::SeqCst);
    });

    let listener3 = FnListener::new(move |event: &TestEvent| {
        c3.fetch_add((event.value * 3) as usize, Ordering::SeqCst);
    });

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 5,
    };

    listener1.on_event(&event);
    listener2.on_event(&event);
    listener3.on_event(&event);

    assert_eq!(counter1.load(Ordering::SeqCst), 5);
    assert_eq!(counter2.load(Ordering::SeqCst), 10);
    assert_eq!(counter3.load(Ordering::SeqCst), 15);
}

#[test]
fn fn_listener_implements_event_listener_trait() {
    // This test verifies FnListener can be used as an EventListener
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    let listener = FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    // Should be able to add to EventListeners collection
    let mut listeners = EventListeners::new();
    listeners.add(listener);

    assert_eq!(listeners.len(), 1);

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Can also be used directly as EventListener
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter2_clone = Arc::clone(&counter2);

    let listener2 = FnListener::new(move |_: &TestEvent| {
        counter2_clone.fetch_add(1, Ordering::SeqCst);
    });

    // Call via trait method
    EventListener::on_event(&listener2, &event);
    assert_eq!(counter2.load(Ordering::SeqCst), 1);
}
