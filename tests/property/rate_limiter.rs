//! Property tests for the rate limiter pattern.
//!
//! Invariants tested:
//! - Never exceeds configured rate within a period
//! - Permits refresh after period expires
//! - Different window types maintain their guarantees

use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Property: Fixed window rate limiter never allows more than limit_for_period requests
    #[test]
    fn fixed_window_respects_limit(
        limit in 1usize..=50,
        num_requests in 1usize..=200,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let accepted = Arc::new(AtomicUsize::new(0));

            let accepted_clone = Arc::clone(&accepted);
            let svc = tower::service_fn(move |_req: ()| {
                let accepted = Arc::clone(&accepted_clone);
                async move {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(())
                }
            });

            let layer = RateLimiterLayer::builder()
                .limit_for_period(limit)
                .refresh_period(Duration::from_secs(60)) // Long period so no refresh
                .timeout_duration(Duration::from_millis(1))
                .window_type(WindowType::Fixed)
                .build();

            let mut service = layer.layer(svc);

            for _ in 0..num_requests {
                let _ = service.ready().await.unwrap().call(()).await;
            }

            let total_accepted = accepted.load(Ordering::SeqCst);
            prop_assert!(
                total_accepted <= limit,
                "Accepted {} requests but limit was {}",
                total_accepted,
                limit
            );

            Ok(())
        })?;
    }

    /// Property: Sliding log rate limiter never allows more than limit within any window
    #[test]
    fn sliding_log_respects_limit(
        limit in 1usize..=30,
        num_requests in 1usize..=100,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let accepted = Arc::new(AtomicUsize::new(0));

            let accepted_clone = Arc::clone(&accepted);
            let svc = tower::service_fn(move |_req: ()| {
                let accepted = Arc::clone(&accepted_clone);
                async move {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(())
                }
            });

            let layer = RateLimiterLayer::builder()
                .limit_for_period(limit)
                .refresh_period(Duration::from_secs(60))
                .timeout_duration(Duration::from_millis(1))
                .window_type(WindowType::SlidingLog)
                .build();

            let mut service = layer.layer(svc);

            for _ in 0..num_requests {
                let _ = service.ready().await.unwrap().call(()).await;
            }

            let total_accepted = accepted.load(Ordering::SeqCst);
            prop_assert!(
                total_accepted <= limit,
                "Accepted {} requests but limit was {}",
                total_accepted,
                limit
            );

            Ok(())
        })?;
    }

    /// Property: Sliding counter rate limiter respects approximate limit
    #[test]
    fn sliding_counter_respects_approximate_limit(
        limit in 5usize..=50,
        num_requests in 1usize..=100,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let accepted = Arc::new(AtomicUsize::new(0));

            let accepted_clone = Arc::clone(&accepted);
            let svc = tower::service_fn(move |_req: ()| {
                let accepted = Arc::clone(&accepted_clone);
                async move {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(())
                }
            });

            let layer = RateLimiterLayer::builder()
                .limit_for_period(limit)
                .refresh_period(Duration::from_secs(60))
                .timeout_duration(Duration::from_millis(1))
                .window_type(WindowType::SlidingCounter)
                .build();

            let mut service = layer.layer(svc);

            for _ in 0..num_requests {
                let _ = service.ready().await.unwrap().call(()).await;
            }

            let total_accepted = accepted.load(Ordering::SeqCst);
            // Sliding counter is approximate, allow some slack
            prop_assert!(
                total_accepted <= limit + 1,
                "Accepted {} requests but limit was {} (sliding counter allows +1)",
                total_accepted,
                limit
            );

            Ok(())
        })?;
    }

    /// Property: Rate limiter refreshes permits after period
    #[test]
    fn rate_limiter_refreshes_permits(
        limit in 1usize..=20,
        window_type in prop_oneof![
            Just(WindowType::Fixed),
            Just(WindowType::SlidingLog),
            Just(WindowType::SlidingCounter),
        ],
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let accepted = Arc::new(AtomicUsize::new(0));

            let accepted_clone = Arc::clone(&accepted);
            let svc = tower::service_fn(move |_req: ()| {
                let accepted = Arc::clone(&accepted_clone);
                async move {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(())
                }
            });

            // Short period for refresh test
            let layer = RateLimiterLayer::builder()
                .limit_for_period(limit)
                .refresh_period(Duration::from_millis(50))
                .timeout_duration(Duration::from_millis(1))
                .window_type(window_type)
                .build();

            let mut service = layer.layer(svc);

            // Exhaust first period
            for _ in 0..limit {
                let _ = service.ready().await.unwrap().call(()).await;
            }

            let first_period = accepted.load(Ordering::SeqCst);
            prop_assert_eq!(first_period, limit);

            // Wait for refresh
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Should be able to make more requests
            for _ in 0..limit {
                let _ = service.ready().await.unwrap().call(()).await;
            }

            let after_refresh = accepted.load(Ordering::SeqCst);
            prop_assert!(
                after_refresh > first_period,
                "No permits refreshed: before={} after={}",
                first_period,
                after_refresh
            );

            Ok(())
        })?;
    }

    /// Property: Concurrent requests are properly serialized for rate limiting
    #[test]
    fn rate_limiter_handles_concurrent_requests(
        limit in 5usize..=20,
        num_concurrent in 10usize..=50,
    ) {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let accepted = Arc::new(AtomicUsize::new(0));

            let accepted_clone = Arc::clone(&accepted);
            let svc = tower::service_fn(move |_req: ()| {
                let accepted = Arc::clone(&accepted_clone);
                async move {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(())
                }
            });

            let layer = RateLimiterLayer::builder()
                .limit_for_period(limit)
                .refresh_period(Duration::from_secs(60))
                .timeout_duration(Duration::from_millis(10))
                .build();

            let service = layer.layer(svc);

            // Spawn concurrent requests
            let mut handles = vec![];
            for _ in 0..num_concurrent {
                let mut svc = service.clone();
                handles.push(tokio::spawn(async move {
                    let _ = svc.ready().await.unwrap().call(()).await;
                }));
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let total_accepted = accepted.load(Ordering::SeqCst);
            prop_assert!(
                total_accepted <= limit,
                "Concurrent requests bypassed limit: accepted {} but limit was {}",
                total_accepted,
                limit
            );

            Ok(())
        })?;
    }
}
