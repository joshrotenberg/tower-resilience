//! Parity coverage for the upstream sliding-window rate-limit discussion.
//!
//! Tracks the "sliding window" row in `docs/upstream-watchlist.md`, sourced
//! from tower-rs/tower#842 ("[Feature Request]: Sliding Window Ratelimit").
//! That issue documents the fixed-window boundary-burst problem and the
//! guarantee a sliding window is meant to provide instead: for quota `Q` and
//! duration `T`, no more than `Q` calls may land in any span `[now-T, now]`.
//! This crate's `WindowType::SlidingLog` and `WindowType::SlidingCounter`
//! were added in direct response to that issue (see the issue thread for the
//! discussion between the reporter and this crate's maintainer).
//!
//! These tests exercise the public `RateLimiterLayer` API only -- no access
//! to internal state -- and use a paused virtual clock
//! (`#[tokio::test(start_paused = true)]` plus `tokio::time::advance`) so the
//! timing assertions are exact and independent of scheduler speed, following
//! the deterministic style established in `tests/stress/ratelimiter.rs`
//! (PR #405).

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

/// A zero timeout makes admission decisions immediate: the first attempt is
/// always tried regardless of the configured timeout, so `Ok` means a permit
/// was available right now and `Err` means it was not -- with no waiting
/// involved either way.
fn immediate_layer(window_type: WindowType, limit: usize, period: Duration) -> RateLimiterLayer {
    RateLimiterLayer::builder()
        .limit_for_period(limit)
        .refresh_period(period)
        .timeout_duration(Duration::ZERO)
        .window_type(window_type)
        .build()
}

/// tower-rs/tower#842 opens with this exact scenario as the motivating case
/// for a sliding window: "100 requests at t=0.9s (end of window 1) ... 100
/// requests at t=1.1s (start of window 2) ... Results in 200 requests in 0.2
/// seconds." Fixed window is documented (and expected) to allow this.
#[tokio::test(start_paused = true)]
async fn fixed_window_allows_the_upstream_documented_boundary_burst() {
    const LIMIT: usize = 100;
    const PERIOD: Duration = Duration::from_secs(1);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });
    let mut service = immediate_layer(WindowType::Fixed, LIMIT, PERIOD).layer(svc);

    tokio::time::advance(Duration::from_millis(900)).await;
    for i in 0..LIMIT as u32 {
        assert!(
            service.ready().await.unwrap().call(i).await.is_ok(),
            "first batch request {i} should be admitted"
        );
    }

    tokio::time::advance(Duration::from_millis(200)).await;
    for i in LIMIT as u32..(2 * LIMIT) as u32 {
        assert!(
            service.ready().await.unwrap().call(i).await.is_ok(),
            "second batch request {i} should be admitted across the refreshed window"
        );
    }

    // 200 requests admitted inside a 200ms span -- exactly the boundary
    // burst upstream describes, and exactly what the sliding window types
    // below prevent or bound.
}

/// The same scenario against `SlidingLog`: the guarantee upstream asks for
/// -- no more than `LIMIT` calls in any trailing `PERIOD` -- holds even
/// across the window boundary.
#[tokio::test(start_paused = true)]
async fn sliding_log_prevents_the_upstream_documented_boundary_burst() {
    const LIMIT: usize = 100;
    const PERIOD: Duration = Duration::from_secs(1);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });
    let mut service = immediate_layer(WindowType::SlidingLog, LIMIT, PERIOD).layer(svc);

    tokio::time::advance(Duration::from_millis(900)).await;
    for i in 0..LIMIT as u32 {
        assert!(
            service.ready().await.unwrap().call(i).await.is_ok(),
            "first batch request {i} should be admitted"
        );
    }

    // 200ms later the first batch's timestamps (t=900ms) are still inside
    // the trailing 1s window (800ms..1900ms), so none of the second batch
    // can be admitted yet -- unlike fixed window's full refresh.
    tokio::time::advance(Duration::from_millis(200)).await;
    let result = service.ready().await.unwrap().call(LIMIT as u32).await;
    assert!(
        result.is_err(),
        "sliding log must not allow a second full batch at the boundary"
    );

    // Once the first batch's entries fully age out of the window (900ms +
    // 1000ms = 1900ms, i.e. 800ms further), capacity returns.
    tokio::time::advance(Duration::from_millis(800)).await;
    for i in (LIMIT as u32)..(2 * LIMIT) as u32 {
        assert!(
            service.ready().await.unwrap().call(i).await.is_ok(),
            "request {i} should be admitted once the first batch has aged out"
        );
    }
}

/// `SlidingCounter` trades exactness for O(1) memory: it blends the previous
/// and current bucket with a weighted average (see the `WindowType::SlidingCounter`
/// docs) rather than keeping a hard timestamp log. Parity here isn't "matches
/// SlidingLog exactly" -- it's that the approximation lands strictly between
/// fixed window's full boundary burst and sliding log's zero burst, per the
/// documented weighted formula.
#[tokio::test(start_paused = true)]
async fn sliding_counter_bounds_the_boundary_burst_between_fixed_and_sliding_log() {
    const LIMIT: usize = 100;
    const PERIOD: Duration = Duration::from_secs(1);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });
    let mut service = immediate_layer(WindowType::SlidingCounter, LIMIT, PERIOD).layer(svc);

    // Fill the first bucket completely.
    for i in 0..LIMIT as u32 {
        assert!(service.ready().await.unwrap().call(i).await.is_ok());
    }

    // Half a bucket into the next window: previous_weight = 0.5, so the
    // weighted formula documented on `WindowType::SlidingCounter`
    // (`previous_count * (1 - elapsed_ratio) + current_count`) admits
    // exactly LIMIT/2 more before the weighted count reaches LIMIT again.
    tokio::time::advance(Duration::from_millis(1_500)).await;

    let mut admitted_in_second_batch: usize = 0;
    for i in LIMIT as u32..(2 * LIMIT) as u32 {
        if service.ready().await.unwrap().call(i).await.is_ok() {
            admitted_in_second_batch += 1;
        }
    }

    assert_eq!(
        admitted_in_second_batch,
        LIMIT / 2,
        "sliding counter should admit exactly half of the second batch \
         at 50% bucket decay -- more than sliding log's zero, less than \
         fixed window's full second batch"
    );
}

/// Direct check of the guarantee upstream's issue states in its own words:
/// "there hasn't been more than Q calls during the span `[now-T, now]`."
/// This walks a paused clock through several window rotations and verifies
/// the invariant holds for the actually-admitted timestamps, using only the
/// public API -- an independent black-box check, not a re-derivation of the
/// crate's own bookkeeping.
#[tokio::test(start_paused = true)]
async fn sliding_log_upholds_the_upstream_stated_invariant_across_many_windows() {
    const LIMIT: usize = 3;
    const WINDOW: Duration = Duration::from_millis(100);
    const STEP: Duration = Duration::from_millis(25);
    const ATTEMPTS: u32 = 40; // spans 10 windows at STEP granularity

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });
    let mut service = immediate_layer(WindowType::SlidingLog, LIMIT, WINDOW).layer(svc);

    let mut admitted_at = Vec::new();
    let mut elapsed = Duration::ZERO;

    for i in 0..ATTEMPTS {
        if service.ready().await.unwrap().call(i).await.is_ok() {
            admitted_at.push(elapsed);
        }
        tokio::time::advance(STEP).await;
        elapsed += STEP;
    }

    assert!(
        admitted_at.len() >= LIMIT,
        "sanity: the limiter should admit at least a full window's worth"
    );

    for (index, &timestamp) in admitted_at.iter().enumerate() {
        let window_start = timestamp.saturating_sub(WINDOW);
        let in_window = admitted_at[..=index]
            .iter()
            .filter(|&&candidate| candidate > window_start)
            .count();
        assert!(
            in_window <= LIMIT,
            "upstream invariant violated: {in_window} calls admitted within \
             ({window_start:?}, {timestamp:?}], limit is {LIMIT}"
        );
    }
}

/// `SlidingLog` expires entries once `age >= window_duration` -- a half-open
/// convention where an entry exactly `window_duration` old is expired, not
/// one nanosecond older. This is the precise "window boundary behavior"
/// edge case a sliding-window implementation has to get right, and the exact
/// scenario upstream's issue asks for ("no more than Q calls during the
/// span"): the boundary itself must not double-count.
#[tokio::test(start_paused = true)]
async fn sliding_log_boundary_expiry_is_exact_not_approximate() {
    const LIMIT: usize = 1;
    const WINDOW: Duration = Duration::from_millis(100);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });
    let mut service = immediate_layer(WindowType::SlidingLog, LIMIT, WINDOW).layer(svc);

    assert!(service.ready().await.unwrap().call(0).await.is_ok());

    // One nanosecond short of the window duration: the entry must still
    // count against the limit.
    tokio::time::advance(WINDOW - Duration::from_nanos(1)).await;
    assert!(
        service.ready().await.unwrap().call(1).await.is_err(),
        "an entry 1ns younger than the window duration must still be counted"
    );

    // Exactly the window duration: the entry must be expired.
    tokio::time::advance(Duration::from_nanos(1)).await;
    assert!(
        service.ready().await.unwrap().call(2).await.is_ok(),
        "an entry exactly window_duration old must be expired"
    );
}
