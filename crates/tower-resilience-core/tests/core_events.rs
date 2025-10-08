//! P0 Event System Core Tests
//!
//! Tests for the core event system functionality including:
//! - Empty collections
//! - Adding and emitting events
//! - Multiple listeners
//! - Event data verification
//! - Large numbers of listeners

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tower_resilience_core::events::{EventListeners, FnListener, ResilienceEvent};

#[derive(Debug, Clone)]
struct TestEvent {
    name: String,
    timestamp: Instant,
    event_type: &'static str,
}

impl ResilienceEvent for TestEvent {
    fn event_type(&self) -> &'static str {
        self.event_type
    }

    fn timestamp(&self) -> Instant {
        self.timestamp
    }

    fn pattern_name(&self) -> &str {
        &self.name
    }
}

#[test]
fn empty_listeners_collection() {
    let listeners: EventListeners<TestEvent> = EventListeners::new();
    assert!(listeners.is_empty());
    assert_eq!(listeners.len(), 0);
}

#[test]
fn add_listener_increases_len() {
    let mut listeners = EventListeners::new();
    assert_eq!(listeners.len(), 0);

    listeners.add(FnListener::new(|_: &TestEvent| {}));
    assert_eq!(listeners.len(), 1);
    assert!(!listeners.is_empty());

    listeners.add(FnListener::new(|_: &TestEvent| {}));
    assert_eq!(listeners.len(), 2);

    listeners.add(FnListener::new(|_: &TestEvent| {}));
    assert_eq!(listeners.len(), 3);
}

#[test]
fn emit_to_empty_listeners_does_not_panic() {
    let listeners: EventListeners<TestEvent> = EventListeners::new();
    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        event_type: "test_event",
    };

    // Should not panic
    listeners.emit(&event);
}

#[test]
fn single_listener_receives_event() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        event_type: "test_event",
    };

    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn multiple_listeners_all_called() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);
    let c3 = Arc::clone(&counter3);

    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c1.fetch_add(1, Ordering::SeqCst);
    }));
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c2.fetch_add(2, Ordering::SeqCst);
    }));
    listeners.add(FnListener::new(move |_: &TestEvent| {
        c3.fetch_add(3, Ordering::SeqCst);
    }));

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        event_type: "test_event",
    };

    listeners.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 2);
    assert_eq!(counter3.load(Ordering::SeqCst), 3);

    // Emit again to verify listeners can be called multiple times
    listeners.emit(&event);
    assert_eq!(counter1.load(Ordering::SeqCst), 2);
    assert_eq!(counter2.load(Ordering::SeqCst), 4);
    assert_eq!(counter3.load(Ordering::SeqCst), 6);
}

#[test]
fn listener_receives_correct_event_data() {
    let received_name = Arc::new(std::sync::Mutex::new(String::new()));
    let received_type = Arc::new(std::sync::Mutex::new(String::new()));
    let received_timestamp = Arc::new(std::sync::Mutex::new(None));

    let name_clone = Arc::clone(&received_name);
    let type_clone = Arc::clone(&received_type);
    let timestamp_clone = Arc::clone(&received_timestamp);

    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(move |event: &TestEvent| {
        *name_clone.lock().unwrap() = event.pattern_name().to_string();
        *type_clone.lock().unwrap() = event.event_type().to_string();
        *timestamp_clone.lock().unwrap() = Some(event.timestamp());
    }));

    let now = Instant::now();
    let event = TestEvent {
        name: "circuit-breaker-1".to_string(),
        timestamp: now,
        event_type: "state_transition",
    };

    listeners.emit(&event);

    assert_eq!(*received_name.lock().unwrap(), "circuit-breaker-1");
    assert_eq!(*received_type.lock().unwrap(), "state_transition");
    assert_eq!(received_timestamp.lock().unwrap().unwrap(), now);
}

#[test]
fn event_with_different_data_types() {
    #[derive(Debug)]
    struct ComplexEvent {
        name: String,
        timestamp: Instant,
        value: i64,
        flag: bool,
        data: Vec<u8>,
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

    let received_value = Arc::new(AtomicUsize::new(0));
    let received_flag = Arc::new(std::sync::Mutex::new(false));
    let received_data = Arc::new(std::sync::Mutex::new(Vec::new()));

    let value_clone = Arc::clone(&received_value);
    let flag_clone = Arc::clone(&received_flag);
    let data_clone = Arc::clone(&received_data);

    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(move |event: &ComplexEvent| {
        received_value.store(event.value as usize, Ordering::SeqCst);
        *flag_clone.lock().unwrap() = event.flag;
        *data_clone.lock().unwrap() = event.data.clone();
    }));

    let event = ComplexEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 42,
        flag: true,
        data: vec![1, 2, 3, 4, 5],
    };

    listeners.emit(&event);

    assert_eq!(value_clone.load(Ordering::SeqCst), 42);
    assert_eq!(*received_flag.lock().unwrap(), true);
    assert_eq!(*received_data.lock().unwrap(), vec![1, 2, 3, 4, 5]);
}

#[test]
fn default_trait_creates_empty_collection() {
    let listeners: EventListeners<TestEvent> = EventListeners::default();
    assert!(listeners.is_empty());
    assert_eq!(listeners.len(), 0);

    // Should work the same as new()
    let listeners2: EventListeners<TestEvent> = EventListeners::new();
    assert_eq!(listeners.len(), listeners2.len());
}

#[test]
fn clone_creates_independent_copy() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    let mut listeners = EventListeners::new();
    listeners.add(FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let listeners_copy = listeners.clone();

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        event_type: "test_event",
    };

    // Both should call the same listener (Arc-shared)
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    listeners_copy.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 2);

    // Adding to one doesn't affect the other
    listeners.add(FnListener::new(|_: &TestEvent| {}));
    assert_eq!(listeners.len(), 2);
    assert_eq!(listeners_copy.len(), 1);
}

#[test]
fn large_number_of_listeners() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut listeners = EventListeners::new();

    // Add 150 listeners
    for i in 0..150 {
        let counter_clone = Arc::clone(&counter);
        listeners.add(FnListener::new(move |_: &TestEvent| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(listeners.len(), i + 1);
    }

    assert_eq!(listeners.len(), 150);
    assert!(!listeners.is_empty());

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        event_type: "test_event",
    };

    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 150);

    // Emit again
    listeners.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), 300);
}
