//! Cache stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::{CacheLayer, EvictionPolicy};

use super::get_memory_usage_mb;

/// Test: Large cache (100k entries)
#[tokio::test]
#[ignore]
async fn stress_large_cache() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let layer = CacheLayer::builder()
        .max_size(100_000)
        .key_extractor(|req: &u32| *req)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // Fill cache with 100k unique keys
    for i in 0..100_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let fill_time = start.elapsed();
    let fills = call_count.load(Ordering::Relaxed);

    // Now hit the cache
    call_count.store(0, Ordering::Relaxed);
    let hit_start = Instant::now();

    for i in 0..100_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let hit_time = hit_start.elapsed();
    let hits = call_count.load(Ordering::Relaxed);

    println!("100k cache entries");
    println!("Fill time: {:?}", fill_time);
    println!("Hit time: {:?}", hit_time);
    println!("Service calls on fill: {}", fills);
    println!("Service calls on hit: {}", hits);

    assert_eq!(fills, 100_000, "All should be misses initially");
    assert_eq!(hits, 0, "All should be hits");
    assert!(hit_time < fill_time / 2, "Hits should be much faster");
}

/// Test: High eviction churn
#[tokio::test]
#[ignore]
async fn stress_eviction_churn() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let layer = CacheLayer::builder()
        .max_size(1000)
        .eviction_policy(EvictionPolicy::Lru)
        .key_extractor(|req: &u32| *req)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // Access 10k unique keys with cache size 1000 = lots of evictions
    for i in 0..10_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let calls = call_count.load(Ordering::Relaxed);

    println!("10k keys with 1k cache (high churn)");
    println!("Completed in: {:?}", elapsed);
    println!("Service calls: {}", calls);
    println!("Hit rate: {:.1}%", (1.0 - calls as f64 / 10_000.0) * 100.0);

    assert_eq!(calls, 10_000, "All should be misses due to churn");
}

/// Test: TTL expiration under load
#[tokio::test]
#[ignore]
async fn stress_ttl_expiration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let layer = CacheLayer::builder()
        .max_size(1000)
        .ttl(Duration::from_millis(100))
        .key_extractor(|req: &u32| *req)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let mut total_calls = 0;

    // Run for 2 seconds, repeatedly accessing same 100 keys
    while start.elapsed() < Duration::from_secs(2) {
        for i in 0..100 {
            let _ = service.ready().await.unwrap().call(i).await;
            total_calls += 1;
        }
        // Small delay to allow TTL expiration
        sleep(Duration::from_millis(50)).await;
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);

    println!("TTL expiration test over {:?}", elapsed);
    println!("Total requests: {}", total_calls);
    println!("Service calls: {}", service_calls);
    println!(
        "Hit rate: {:.1}%",
        (1.0 - service_calls as f64 / total_calls as f64) * 100.0
    );

    // With 100ms TTL and 50ms sleep, should see many expirations
    assert!(service_calls > 100, "Should see TTL expirations");
    assert!(service_calls < total_calls, "Should have some hits");
}

/// Test: Compare eviction policies under stress
#[tokio::test]
#[ignore]
async fn stress_eviction_policy_comparison() {
    for policy in [
        EvictionPolicy::Lru,
        EvictionPolicy::Lfu,
        EvictionPolicy::Fifo,
    ] {
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&call_count);

        let svc = tower::service_fn(move |req: u32| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>(format!("response-{}", req))
            }
        });

        let layer = CacheLayer::builder()
            .max_size(100)
            .eviction_policy(policy)
            .key_extractor(|req: &u32| *req)
            .build();

        let mut service = layer.layer(svc);

        let start = Instant::now();

        // Access pattern: 80% hot keys, 20% random
        for i in 0..10_000 {
            let key = if i % 5 == 0 {
                // 20% random cold keys
                1000 + (i % 500)
            } else {
                // 80% hot keys (repeated)
                i % 50
            };
            let _ = service.ready().await.unwrap().call(key).await;
        }

        let elapsed = start.elapsed();
        let calls = call_count.load(Ordering::Relaxed);
        let hit_rate = (1.0 - calls as f64 / 10_000.0) * 100.0;

        println!(
            "{:?}: {} service calls, {:.1}% hit rate in {:?}",
            policy, calls, hit_rate, elapsed
        );

        // LFU should perform best with this access pattern
    }
}

/// Test: Concurrent cache access
#[tokio::test]
#[ignore]
async fn stress_concurrent_access() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let layer = CacheLayer::builder()
        .max_size(1000)
        .key_extractor(|req: &u32| *req)
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 1000 concurrent requests accessing same 100 keys
    for i in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            let key = i % 100;
            svc.ready().await.unwrap().call(key).await
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);
    let hit_rate = (1.0 - service_calls as f64 / 1000.0) * 100.0;

    println!("1000 concurrent requests");
    println!("Completed in: {:?}", elapsed);
    println!("Service calls: {}", service_calls);
    println!("Hit rate: {:.1}%", hit_rate);

    // Should see high hit rate
    assert!(service_calls < 500, "Should have many cache hits");
}

/// Test: Memory usage with different cache sizes
#[tokio::test]
#[ignore]
async fn stress_memory_scaling() {
    for size in [1_000, 10_000, 50_000, 100_000] {
        let mem_start = get_memory_usage_mb();

        let svc =
            tower::service_fn(|req: u32| async move { Ok::<_, ()>(format!("response-{}", req)) });

        let layer = CacheLayer::builder()
            .max_size(size)
            .key_extractor(|req: &u32| *req)
            .build();

        let mut service = layer.layer(svc);

        // Fill the cache
        for i in 0..size {
            let _ = service.ready().await.unwrap().call(i as u32).await;
        }

        let mem_end = get_memory_usage_mb();
        let mem_delta = mem_end - mem_start;

        println!("Cache size {}: {:.2} MB", size, mem_delta);

        if mem_delta > 0.0 {
            // Memory should scale roughly linearly
            let mb_per_entry = mem_delta / size as f64;
            println!("  ~{:.4} MB per entry", mb_per_entry);
        }
    }
}

/// Test: Cache with very fast operations (throughput)
#[tokio::test]
#[ignore]
async fn stress_throughput() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(req)
        }
    });

    let layer = CacheLayer::builder()
        .max_size(1000)
        .key_extractor(|req: &u32| *req)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // 100k requests hitting same 100 keys = high hit rate
    for i in 0..100_000 {
        let key = i % 100;
        let _ = service.ready().await.unwrap().call(key).await;
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);
    let throughput = 100_000.0 / elapsed.as_secs_f64();

    println!("100k requests in {:?}", elapsed);
    println!("Throughput: {:.0} req/sec", throughput);
    println!(
        "Service calls: {} (hit rate: {:.1}%)",
        service_calls,
        (1.0 - service_calls as f64 / 100_000.0) * 100.0
    );

    assert!(throughput > 10_000.0, "Should achieve high throughput");
    assert!(service_calls < 200, "Should have very high hit rate");
}

/// Test: Stability over extended period
#[tokio::test]
#[ignore]
async fn stress_stability() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let layer = CacheLayer::builder()
        .max_size(1000)
        .ttl(Duration::from_millis(500))
        .eviction_policy(EvictionPolicy::Lru)
        .key_extractor(|req: &u32| *req)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let mut total_requests = 0;
    let mut mem_samples = vec![];

    // Run for 10 seconds
    while start.elapsed() < Duration::from_secs(10) {
        // Random access pattern
        for _ in 0..100 {
            let key = (total_requests % 500) as u32;
            let _ = service.ready().await.unwrap().call(key).await;
            total_requests += 1;
        }

        // Sample memory
        let mem = get_memory_usage_mb();
        if mem > 0.0 {
            mem_samples.push(mem);
        }

        sleep(Duration::from_millis(10)).await;
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);

    println!(
        "Stability test: {} requests over {:?}",
        total_requests, elapsed
    );
    println!("Service calls: {}", service_calls);

    if !mem_samples.is_empty() {
        let mem_max = mem_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let mem_min = mem_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let mem_avg = mem_samples.iter().sum::<f64>() / mem_samples.len() as f64;

        println!(
            "Memory: min={:.2} MB, avg={:.2} MB, max={:.2} MB",
            mem_min, mem_avg, mem_max
        );

        // Memory should be stable (not growing unbounded)
        assert!(mem_max - mem_min < 50.0, "Memory should be stable");
    }
}
