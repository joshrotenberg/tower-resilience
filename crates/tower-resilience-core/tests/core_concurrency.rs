//! P0 Thread Safety Tests
//!
//! Tests for thread safety and concurrency including:
//! - Send and Sync trait bounds
//! - Concurrent emissions
//! - Concurrent listener additions
//! - Data race prevention
//! - High concurrency stress tests

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use tower_resilience_core::events::{EventListeners, FnListener, ResilienceEvent};

#[derive(Debug, Clone)]
struct TestEvent {
    name: String,
    timestamp: Instant,
    value: usize,
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
fn event_listeners_is_send() {
    // Compile-time check that EventListeners is Send
    fn assert_send<T: Send>() {}
    assert_send::<EventListeners<TestEvent>>();

    // Runtime verification - can be sent to another thread
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    listeners.add(FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let handle = thread::spawn(move || {
        let event = TestEvent {
            name: "test".to_string(),
            timestamp: Instant::now(),
            value: 0,
        };
        listeners.emit(&event);
    });

    handle.join().unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn event_listeners_is_sync() {
    // Compile-time check that EventListeners is Sync
    fn assert_sync<T: Sync>() {}
    assert_sync::<EventListeners<TestEvent>>();

    // Runtime verification - can be shared across threads via Arc
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    listeners.add(FnListener::new(move |_: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let listeners = Arc::new(listeners);

    let mut handles = vec![];
    for i in 0..5 {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("test-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            listeners_clone.emit(&event);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[test]
fn emit_from_multiple_threads_concurrently() {
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let values = Arc::new(Mutex::new(Vec::new()));

    let counter_clone = Arc::clone(&counter);
    let values_clone = Arc::clone(&values);

    listeners.add(FnListener::new(move |event: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        values_clone.lock().unwrap().push(event.value);
    }));

    let listeners = Arc::new(listeners);
    let num_threads = 10;

    let mut handles = vec![];
    for i in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("thread-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            listeners_clone.emit(&event);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::SeqCst), num_threads);

    let values_vec = values.lock().unwrap();
    assert_eq!(values_vec.len(), num_threads);

    // All values should be present (order may vary)
    let mut sorted_values = values_vec.clone();
    sorted_values.sort();
    assert_eq!(sorted_values, (0..num_threads).collect::<Vec<_>>());
}

#[test]
fn add_listeners_from_multiple_threads() {
    // Need Mutex wrapper for adding listeners concurrently
    let listeners = Arc::new(Mutex::new(EventListeners::new()));
    let counter = Arc::new(AtomicUsize::new(0));

    let num_threads = 10;
    let mut handles = vec![];

    for _ in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let counter_clone = Arc::clone(&counter);

        let handle = thread::spawn(move || {
            let c = Arc::clone(&counter_clone);
            let listener = FnListener::new(move |_: &TestEvent| {
                c.fetch_add(1, Ordering::SeqCst);
            });
            listeners_clone.lock().unwrap().add(listener);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let listeners_locked = listeners.lock().unwrap();
    assert_eq!(listeners_locked.len(), num_threads);

    let event = TestEvent {
        name: "test".to_string(),
        timestamp: Instant::now(),
        value: 0,
    };

    listeners_locked.emit(&event);
    assert_eq!(counter.load(Ordering::SeqCst), num_threads);
}

#[test]
fn concurrent_emit_calls_work_correctly() {
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));

    // Add multiple listeners
    for _ in 0..5 {
        let counter_clone = Arc::clone(&counter);
        listeners.add(FnListener::new(move |_: &TestEvent| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
    }

    let listeners = Arc::new(listeners);
    let num_threads = 20;

    let mut handles = vec![];
    for i in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("test-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            // Each thread emits an event
            listeners_clone.emit(&event);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Each of 20 threads emitted an event to 5 listeners
    assert_eq!(counter.load(Ordering::SeqCst), num_threads * 5);
}

#[test]
fn no_data_races() {
    // This test verifies that concurrent access doesn't cause data races
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let sum = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&counter);
    let sum_clone = Arc::clone(&sum);

    listeners.add(FnListener::new(move |event: &TestEvent| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        sum_clone.fetch_add(event.value, Ordering::SeqCst);
    }));

    let listeners = Arc::new(listeners);
    let num_threads = 50;

    let mut handles = vec![];
    for i in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("test-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            listeners_clone.emit(&event);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::SeqCst), num_threads);

    // Sum should be 0 + 1 + 2 + ... + 49 = 1225
    let expected_sum: usize = (0..num_threads).sum();
    assert_eq!(sum.load(Ordering::SeqCst), expected_sum);
}

#[test]
fn hundred_threads_emitting_simultaneously() {
    let mut listeners = EventListeners::new();
    let counter = Arc::new(AtomicUsize::new(0));

    // Add 10 listeners
    for _ in 0..10 {
        let counter_clone = Arc::clone(&counter);
        listeners.add(FnListener::new(move |_: &TestEvent| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
    }

    let listeners = Arc::new(listeners);
    let num_threads = 100;

    let mut handles = vec![];
    for i in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("stress-test-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            // Each thread emits multiple times
            for _ in 0..5 {
                listeners_clone.emit(&event);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // 100 threads * 5 emissions * 10 listeners = 5000
    assert_eq!(counter.load(Ordering::SeqCst), num_threads * 5 * 10);
}

#[test]
fn thread_safety_with_fn_listener_closures() {
    // Verify that FnListener closures are thread-safe
    struct SharedData {
        count: AtomicUsize,
        values: Mutex<Vec<usize>>,
    }

    let shared = Arc::new(SharedData {
        count: AtomicUsize::new(0),
        values: Mutex::new(Vec::new()),
    });

    let mut listeners = EventListeners::new();

    let shared_clone = Arc::clone(&shared);
    listeners.add(FnListener::new(move |event: &TestEvent| {
        shared_clone.count.fetch_add(1, Ordering::SeqCst);
        shared_clone.values.lock().unwrap().push(event.value);
    }));

    let listeners = Arc::new(listeners);
    let num_threads = 25;

    let mut handles = vec![];
    for i in 0..num_threads {
        let listeners_clone = Arc::clone(&listeners);
        let handle = thread::spawn(move || {
            let event = TestEvent {
                name: format!("test-{}", i),
                timestamp: Instant::now(),
                value: i,
            };
            listeners_clone.emit(&event);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(shared.count.load(Ordering::SeqCst), num_threads);

    let values = shared.values.lock().unwrap();
    assert_eq!(values.len(), num_threads);

    // Verify all values are present
    let mut sorted_values = values.clone();
    sorted_values.sort();
    assert_eq!(sorted_values, (0..num_threads).collect::<Vec<_>>());
}
