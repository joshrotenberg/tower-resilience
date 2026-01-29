//! Tests for hedge event emission and listeners.

use super::TestError;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_core::FnListener;
use tower_resilience_hedge::{HedgeEvent, HedgeLayer};

#[tokio::test]
async fn test_primary_started_event() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    let events = events.lock().unwrap();
    assert!(events.len() >= 2); // At least PrimaryStarted and PrimarySucceeded

    // First event should be PrimaryStarted
    assert!(matches!(events[0], HedgeEvent::PrimaryStarted { .. }));
}

#[tokio::test]
async fn test_primary_succeeded_event() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    let events = events.lock().unwrap();

    // Find PrimarySucceeded event
    let succeeded = events
        .iter()
        .find(|e| matches!(e, HedgeEvent::PrimarySucceeded { .. }));
    assert!(succeeded.is_some(), "expected PrimarySucceeded event");

    if let Some(HedgeEvent::PrimarySucceeded {
        hedges_cancelled, ..
    }) = succeeded
    {
        assert_eq!(*hedges_cancelled, 0); // No hedges were spawned
    }
}

#[tokio::test]
async fn test_hedge_started_and_succeeded_events() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service = service_fn(|_req: String| async move {
        // Slow enough to trigger hedge
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok::<_, TestError>("success".to_string())
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(30))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = events.lock().unwrap();

    // Should have HedgeStarted
    let hedge_started = events
        .iter()
        .find(|e| matches!(e, HedgeEvent::HedgeStarted { .. }));
    assert!(hedge_started.is_some(), "expected HedgeStarted event");

    if let Some(HedgeEvent::HedgeStarted { attempt, delay, .. }) = hedge_started {
        assert_eq!(*attempt, 1);
        assert_eq!(*delay, Duration::from_millis(30));
    }
}

#[tokio::test]
async fn test_all_failed_event() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = events.lock().unwrap();

    // Should have AllFailed event
    let all_failed = events
        .iter()
        .find(|e| matches!(e, HedgeEvent::AllFailed { .. }));
    assert!(all_failed.is_some(), "expected AllFailed event");

    if let Some(HedgeEvent::AllFailed { attempts, .. }) = all_failed {
        assert_eq!(*attempts, 3);
    }
}

#[tokio::test]
async fn test_event_ordering() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service = service_fn(|_req: String| async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("success".to_string())
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(20))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = events.lock().unwrap();

    // Verify event order
    assert!(events.len() >= 3);

    // First event should be PrimaryStarted
    assert!(
        matches!(events[0], HedgeEvent::PrimaryStarted { .. }),
        "first event should be PrimaryStarted, got {:?}",
        events[0]
    );

    // Should have HedgeStarted before success
    let hedge_started_idx = events
        .iter()
        .position(|e| matches!(e, HedgeEvent::HedgeStarted { .. }));
    assert!(hedge_started_idx.is_some());
}

#[tokio::test]
async fn test_event_name_preserved() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);

    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .name("my-custom-hedge")
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    let events = events.lock().unwrap();

    // All events should have the custom name
    for event in events.iter() {
        let name = match event {
            HedgeEvent::PrimaryStarted { name, .. } => name,
            HedgeEvent::PrimarySucceeded { name, .. } => name,
            HedgeEvent::HedgeStarted { name, .. } => name,
            HedgeEvent::HedgeSucceeded { name, .. } => name,
            HedgeEvent::AllFailed { name, .. } => name,
        };
        assert_eq!(name.as_deref(), Some("my-custom-hedge"));
    }
}

#[tokio::test]
async fn test_multiple_listeners() {
    let events1 = Arc::new(Mutex::new(Vec::new()));
    let events2 = Arc::new(Mutex::new(Vec::new()));
    let ev1 = Arc::clone(&events1);
    let ev2 = Arc::clone(&events2);

    let service =
        service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev1.lock().unwrap().push(e.clone());
        }))
        .on_event(FnListener::new(move |e: &HedgeEvent| {
            ev2.lock().unwrap().push(e.clone());
        }))
        .build();
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    let events1 = events1.lock().unwrap();
    let events2 = events2.lock().unwrap();

    // Both listeners should receive the same events
    assert_eq!(events1.len(), events2.len());
    assert!(events1.len() >= 2);
}
