//! Concurrency tests for coalesce pattern.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_coalesce::CoalesceLayer;

#[tokio::test]
async fn test_concurrent_requests_coalesce() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            // Simulate slow operation
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn multiple concurrent requests with the same key
    let mut handles = vec![];
    for _ in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call("same-key".to_string())
                .await
        }));
    }

    // All should succeed with the same response
    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result.unwrap(), "response: same-key");
    }

    // But only one actual call was made
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_different_keys_execute_separately() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn requests with different keys
    let mut handles = vec![];
    for i in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(format!("key-{}", i)).await
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        assert_eq!(result.unwrap(), format!("response: key-{}", i));
    }

    // Each unique key caused a separate call
    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_error_propagates_to_all_waiters() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Err::<String, _>(TestError::new("shared error"))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn multiple concurrent requests
    let mut handles = vec![];
    for _ in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call("same-key".to_string())
                .await
        }));
    }

    // All should receive the error
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("shared error"));
    }

    // But only one call was made
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_mixed_keys_concurrent() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn requests - some with same key, some different
    let mut handles = vec![];

    // 3 requests for "key-a"
    for _ in 0..3 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call("key-a".to_string()).await
        }));
    }

    // 2 requests for "key-b"
    for _ in 0..2 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call("key-b".to_string()).await
        }));
    }

    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Only 2 calls (one per unique key)
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_high_concurrency() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Spawn 100 concurrent requests for the same key
    let mut handles = vec![];
    for _ in 0..100 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call("hot-key".to_string()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result.unwrap(), "response: hot-key");
    }

    // Only 1 call despite 100 concurrent requests
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
