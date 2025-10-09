//! Event system tests for tower-retry-plus.
//!
//! Tests event emission including:
//! - Success event on first try
//! - Retry events with correct attempt numbers
//! - Error event after exhaustion
//! - IgnoredError event for non-retryable errors
//! - Multiple listeners receive events

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::RetryLayer;

#[derive(Debug, Clone)]
struct TestError {
    retryable: bool,
}

#[tokio::test]
async fn success_event_on_first_try() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let retry_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));

    let sc = Arc::clone(&success_count);
    let rc = Arc::clone(&retry_count);
    let ec = Arc::clone(&error_count);

    let service =
        tower::service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let config = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .on_success(move |attempts| {
            sc.fetch_add(1, Ordering::SeqCst);
            assert_eq!(attempts, 1, "Should succeed on first attempt");
        })
        .on_retry(move |_, _| {
            rc.fetch_add(1, Ordering::SeqCst);
        })
        .on_error(move |_| {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(success_count.load(Ordering::SeqCst), 1);
    assert_eq!(retry_count.load(Ordering::SeqCst), 0);
    assert_eq!(error_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn success_event_after_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));
    let retry_count = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let sc = Arc::clone(&success_count);
    let rc = Arc::clone(&retry_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .on_success(move |attempts| {
            sc.fetch_add(1, Ordering::SeqCst);
            assert_eq!(attempts, 3, "Should succeed on third attempt");
        })
        .on_retry(move |_, _| {
            rc.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(success_count.load(Ordering::SeqCst), 1);
    assert_eq!(retry_count.load(Ordering::SeqCst), 2); // 2 retries before success
}

#[tokio::test]
async fn retry_events_with_correct_attempt_numbers() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let retry_attempts = Arc::new(std::sync::Mutex::new(Vec::new()));

    let cc = Arc::clone(&call_count);
    let ra = Arc::clone(&retry_attempts);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .on_retry(move |attempt, _delay| {
            ra.lock().unwrap().push(attempt);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let attempts = retry_attempts.lock().unwrap();
    assert_eq!(*attempts, vec![0, 1, 2]);
}

#[tokio::test]
async fn retry_events_include_delay_information() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let delays = Arc::new(std::sync::Mutex::new(Vec::new()));

    let cc = Arc::clone(&call_count);
    let d = Arc::clone(&delays);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(4)
        .fixed_backoff(Duration::from_millis(50))
        .on_retry(move |_attempt, delay| {
            d.lock().unwrap().push(delay);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let delay_list = delays.lock().unwrap();
    assert_eq!(delay_list.len(), 2);
    // All delays should be 50ms (fixed backoff)
    for delay in delay_list.iter() {
        assert_eq!(*delay, Duration::from_millis(50));
    }
}

#[tokio::test]
async fn error_event_after_exhaustion() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let error_attempts = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&call_count);
    let ec = Arc::clone(&error_count);
    let ea = Arc::clone(&error_attempts);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError { retryable: true })
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .on_error(move |attempts| {
            ec.fetch_add(1, Ordering::SeqCst);
            ea.store(attempts, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
    assert_eq!(error_count.load(Ordering::SeqCst), 1);
    assert_eq!(error_attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn ignored_error_event_for_non_retryable() {
    let ignored_count = Arc::new(AtomicUsize::new(0));
    let retry_count = Arc::new(AtomicUsize::new(0));

    let ic = Arc::clone(&ignored_count);
    let rc = Arc::clone(&retry_count);

    let service = tower::service_fn(|_req: String| async move {
        Err::<String, _>(TestError { retryable: false })
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .retry_on(|e: &TestError| e.retryable)
        .on_ignored_error(move || {
            ic.fetch_add(1, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            rc.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    assert_eq!(ignored_count.load(Ordering::SeqCst), 1);
    assert_eq!(retry_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn multiple_listeners_all_receive_events() {
    let listener1_count = Arc::new(AtomicUsize::new(0));
    let listener2_count = Arc::new(AtomicUsize::new(0));
    let listener3_count = Arc::new(AtomicUsize::new(0));

    let l1 = Arc::clone(&listener1_count);
    let l2 = Arc::clone(&listener2_count);
    let l3 = Arc::clone(&listener3_count);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(4)
        .fixed_backoff(Duration::from_millis(10))
        .on_retry(move |_, _| {
            l1.fetch_add(1, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            l2.fetch_add(1, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            l3.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // All three listeners should have been called for each retry
    assert_eq!(listener1_count.load(Ordering::SeqCst), 2);
    assert_eq!(listener2_count.load(Ordering::SeqCst), 2);
    assert_eq!(listener3_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn all_event_types_in_successful_scenario() {
    let success_fired = Arc::new(AtomicBool::new(false));
    let retry_fired = Arc::new(AtomicBool::new(false));
    let error_fired = Arc::new(AtomicBool::new(false));
    let ignored_fired = Arc::new(AtomicBool::new(false));

    let sf = Arc::clone(&success_fired);
    let rf = Arc::clone(&retry_fired);
    let ef = Arc::clone(&error_fired);
    let igf = Arc::clone(&ignored_fired);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .on_success(move |_| {
            sf.store(true, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            rf.store(true, Ordering::SeqCst);
        })
        .on_error(move |_| {
            ef.store(true, Ordering::SeqCst);
        })
        .on_ignored_error(move || {
            igf.store(true, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // Success and Retry should fire, Error and IgnoredError should not
    assert!(success_fired.load(Ordering::SeqCst));
    assert!(retry_fired.load(Ordering::SeqCst));
    assert!(!error_fired.load(Ordering::SeqCst));
    assert!(!ignored_fired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn all_event_types_in_exhausted_scenario() {
    let success_fired = Arc::new(AtomicBool::new(false));
    let retry_fired = Arc::new(AtomicBool::new(false));
    let error_fired = Arc::new(AtomicBool::new(false));
    let ignored_fired = Arc::new(AtomicBool::new(false));

    let sf = Arc::clone(&success_fired);
    let rf = Arc::clone(&retry_fired);
    let ef = Arc::clone(&error_fired);
    let igf = Arc::clone(&ignored_fired);

    let service =
        tower::service_fn(
            |_req: String| async move { Err::<String, _>(TestError { retryable: true }) },
        );

    let config = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .on_success(move |_| {
            sf.store(true, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            rf.store(true, Ordering::SeqCst);
        })
        .on_error(move |_| {
            ef.store(true, Ordering::SeqCst);
        })
        .on_ignored_error(move || {
            igf.store(true, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // Retry and Error should fire, Success and IgnoredError should not
    assert!(!success_fired.load(Ordering::SeqCst));
    assert!(retry_fired.load(Ordering::SeqCst));
    assert!(error_fired.load(Ordering::SeqCst));
    assert!(!ignored_fired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn all_event_types_in_ignored_scenario() {
    let success_fired = Arc::new(AtomicBool::new(false));
    let retry_fired = Arc::new(AtomicBool::new(false));
    let error_fired = Arc::new(AtomicBool::new(false));
    let ignored_fired = Arc::new(AtomicBool::new(false));

    let sf = Arc::clone(&success_fired);
    let rf = Arc::clone(&retry_fired);
    let ef = Arc::clone(&error_fired);
    let igf = Arc::clone(&ignored_fired);

    let service = tower::service_fn(|_req: String| async move {
        Err::<String, _>(TestError { retryable: false })
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .retry_on(|e: &TestError| e.retryable)
        .on_success(move |_| {
            sf.store(true, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            rf.store(true, Ordering::SeqCst);
        })
        .on_error(move |_| {
            ef.store(true, Ordering::SeqCst);
        })
        .on_ignored_error(move || {
            igf.store(true, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // Only IgnoredError should fire
    assert!(!success_fired.load(Ordering::SeqCst));
    assert!(!retry_fired.load(Ordering::SeqCst));
    assert!(!error_fired.load(Ordering::SeqCst));
    assert!(ignored_fired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn event_listeners_with_shared_state() {
    let event_log = Arc::new(std::sync::Mutex::new(Vec::new()));

    let log1 = Arc::clone(&event_log);
    let log2 = Arc::clone(&event_log);
    let log3 = Arc::clone(&event_log);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError { retryable: true })
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(4)
        .fixed_backoff(Duration::from_millis(10))
        .on_retry(move |attempt, _| {
            log1.lock().unwrap().push(format!("retry:{}", attempt));
        })
        .on_success(move |attempts| {
            log2.lock().unwrap().push(format!("success:{}", attempts));
        })
        .on_error(move |attempts| {
            log3.lock().unwrap().push(format!("error:{}", attempts));
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let log = event_log.lock().unwrap();
    assert_eq!(log.len(), 3); // 2 retries + 1 success
    assert_eq!(log[0], "retry:0");
    assert_eq!(log[1], "retry:1");
    assert_eq!(log[2], "success:3");
}
