//! Concurrency tests for tower-cache.
//!
//! Tests cache behavior under concurrent access patterns, including:
//! - Concurrent reads from cache
//! - Concurrent writes to cache
//! - Mixed read/write operations
//! - Service cloning (Tower requirement)
//! - Thread safety verification

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::task::JoinSet;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::CacheConfig;

#[tokio::test]
async fn concurrent_reads_from_same_cached_item() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call to populate cache
    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Spawn 10 concurrent reads of the same cached item
    let mut tasks = JoinSet::new();
    for _ in 0..10 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call("test".to_string())
                .await
                .unwrap()
        });
    }

    // Wait for all tasks
    while let Some(result) = tasks.join_next().await {
        let response = result.unwrap();
        assert_eq!(response, "Response: test");
    }

    // Inner service should still only have been called once (cache hits)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_writes_with_different_keys() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(100)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let service = layer.layer(service);

    // Spawn 20 concurrent writes with different keys
    let mut tasks = JoinSet::new();
    for i in 0..20 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            let key = format!("key{}", i);
            svc.ready().await.unwrap().call(key.clone()).await.unwrap()
        });
    }

    // Wait for all tasks
    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(result.unwrap());
    }

    // All writes should succeed
    assert_eq!(results.len(), 20);

    // Inner service should have been called 20 times (all cache misses initially)
    assert_eq!(call_count.load(Ordering::SeqCst), 20);

    // Verify all responses are correct
    for i in 0..20 {
        let expected = format!("Response: key{}", i);
        assert!(results.contains(&expected));
    }
}

#[tokio::test]
async fn concurrent_read_write_mix() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(50)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let service = layer.layer(service);

    // Spawn mixed workload: some write new keys, some read existing keys
    let mut tasks = JoinSet::new();

    // 10 writers with unique keys
    for i in 0..10 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            let key = format!("unique{}", i);
            svc.ready().await.unwrap().call(key).await.unwrap()
        });
    }

    // 10 readers of same key (will cache miss first time)
    for _ in 0..10 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call("shared".to_string())
                .await
                .unwrap()
        });
    }

    // Wait for all tasks
    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(result.unwrap());
    }

    assert_eq!(results.len(), 20);

    // Expected calls: 10 unique keys + at least 1 shared key (first time)
    // Due to concurrency, some "shared" requests might race and all miss cache initially
    // So we expect between 11 (ideal) and 20 (worst case - all miss initially) calls
    let calls = call_count.load(Ordering::SeqCst);
    assert!(
        (11..=20).contains(&calls),
        "Expected between 11 and 20 calls, got {}",
        calls
    );
}

#[tokio::test]
async fn cache_service_cloning_preserves_shared_state() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service1 = layer.layer(service);

    // Clone the service (Tower requirement)
    let mut service2 = service1.clone();
    let mut service3 = service1.clone();

    // service1 populates cache
    service1
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // service2 should hit cache
    service2
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // service3 should also hit cache
    service3
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn thread_safety_of_arc_mutex_cache_store() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok::<_, std::io::Error>(req * 2)
        }
    });

    let config = CacheConfig::builder()
        .max_size(100)
        .key_extractor(|req: &u32| *req)
        .build();

    let layer = config.layer();
    let service = layer.layer(service);

    // Spawn 50 tasks accessing cache concurrently
    let mut tasks = JoinSet::new();
    for i in 0..50 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            // Each task makes 2 calls with same key
            let result1 = svc.ready().await.unwrap().call(i).await.unwrap();
            let result2 = svc.ready().await.unwrap().call(i).await.unwrap();
            (i, result1, result2)
        });
    }

    // Collect results
    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(result.unwrap());
    }

    // Verify correctness
    assert_eq!(results.len(), 50);
    for (i, result1, result2) in results {
        assert_eq!(result1, i * 2);
        assert_eq!(result2, i * 2);
    }

    // Each key should have been computed once (second call cached)
    assert_eq!(call_count.load(Ordering::SeqCst), 50);
}

#[tokio::test]
async fn multiple_clones_accessing_simultaneously() {
    let service = tower::service_fn(|req: String| async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, std::io::Error>(format!("Response: {}", req))
    });

    let config = CacheConfig::builder()
        .max_size(20)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let service = layer.layer(service);

    // Create 5 clones
    let clones: Vec<_> = (0..5).map(|_| service.clone()).collect();

    // Each clone makes 10 calls
    let mut tasks = JoinSet::new();
    for (clone_idx, mut svc) in clones.into_iter().enumerate() {
        tasks.spawn(async move {
            let mut local_results = Vec::new();
            for i in 0..10 {
                let key = format!("key{}", i % 5); // Rotate through 5 keys
                let result = svc.ready().await.unwrap().call(key).await.unwrap();
                local_results.push(result);
            }
            (clone_idx, local_results)
        });
    }

    // Wait for all clones to finish
    let mut all_results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        let (_, results) = result.unwrap();
        all_results.extend(results);
    }

    // All calls should succeed
    assert_eq!(all_results.len(), 50);

    // Verify responses are correct
    for result in all_results {
        assert!(result.starts_with("Response: key"));
    }
}

#[tokio::test]
async fn no_data_corruption_under_concurrent_load() {
    let service = tower::service_fn(|req: u64| async move {
        tokio::time::sleep(Duration::from_micros(100)).await;
        Ok::<_, std::io::Error>(req.to_string())
    });

    let config = CacheConfig::builder()
        .max_size(100)
        .key_extractor(|req: &u64| *req)
        .build();

    let layer = config.layer();
    let service = layer.layer(service);

    // High concurrency: 100 tasks, each making multiple calls
    let mut tasks = JoinSet::new();
    for task_id in 0..100 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            let mut task_results = Vec::new();
            // Each task calls with keys 0-9
            for key in 0..10 {
                let result = svc.ready().await.unwrap().call(key).await.unwrap();
                task_results.push((key, result));
            }
            (task_id, task_results)
        });
    }

    // Collect all results
    let mut all_results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        let (_, results) = result.unwrap();
        all_results.extend(results);
    }

    // Verify no corruption: every key should map to correct value
    for (key, value) in all_results {
        assert_eq!(value, key.to_string());
    }
}

#[tokio::test]
async fn concurrent_cache_hits_maintain_correctness() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Slow service to ensure we're testing cache, not just speed
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, std::io::Error>(format!("Computed: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Pre-populate cache with 5 items
    for i in 0..5 {
        service
            .ready()
            .await
            .unwrap()
            .call(format!("item{}", i))
            .await
            .unwrap();
    }
    assert_eq!(call_count.load(Ordering::SeqCst), 5);

    // Now spawn 50 concurrent readers hitting those cached items
    let mut tasks = JoinSet::new();
    for i in 0..50 {
        let mut svc = service.clone();
        tasks.spawn(async move {
            let key = format!("item{}", i % 5); // Rotate through the 5 cached items
            svc.ready().await.unwrap().call(key.clone()).await.unwrap()
        });
    }

    // Collect results
    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results.push(result.unwrap());
    }

    // All reads should succeed
    assert_eq!(results.len(), 50);

    // Verify correctness: all results should be "Computed: item{0-4}"
    for result in results {
        assert!(result.starts_with("Computed: item"));
    }

    // Inner service should still only have been called 5 times (all cache hits)
    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}
