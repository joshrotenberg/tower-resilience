//! Integration tests for the health check wrapper.

use std::time::Duration;
use tower_resilience_healthcheck::{
    HealthCheckWrapper, HealthChecker, HealthStatus, SelectionStrategy,
};

#[derive(Clone)]
struct MockResource {
    name: String,
    healthy: bool,
}

impl MockResource {
    fn new(name: &str, healthy: bool) -> Self {
        Self {
            name: name.to_string(),
            healthy,
        }
    }
}

struct MockChecker;

impl HealthChecker<MockResource> for MockChecker {
    async fn check(&self, resource: &MockResource) -> HealthStatus {
        if resource.healthy {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

fn wrapper_builder() -> HealthCheckWrapper<MockResource, MockChecker> {
    HealthCheckWrapper::builder()
        .with_context(MockResource::new("backend-1", true), "backend-1")
        .with_context(MockResource::new("backend-2", true), "backend-2")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .build()
}

#[tokio::test]
async fn initial_status_is_unknown() {
    let wrapper = wrapper_builder();
    let statuses = wrapper.get_all_statuses().await;
    assert_eq!(statuses.len(), 2);
    for (_, status) in &statuses {
        assert_eq!(*status, HealthStatus::Unknown);
    }
}

#[tokio::test]
async fn healthy_resources_detected_after_check() {
    let wrapper = wrapper_builder();
    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let status = wrapper.get_status("backend-1").await;
    assert_eq!(status, Some(HealthStatus::Healthy));

    let status = wrapper.get_status("backend-2").await;
    assert_eq!(status, Some(HealthStatus::Healthy));

    wrapper.stop().await;
}

#[tokio::test]
async fn unhealthy_resource_excluded_from_get_healthy() {
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("healthy", true), "healthy")
        .with_context(MockResource::new("down", false), "down")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(1)
        .build();

    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resource = wrapper.get_healthy().await;
    assert!(resource.is_some());
    assert_eq!(resource.unwrap().name, "healthy");

    wrapper.stop().await;
}

#[tokio::test]
async fn no_healthy_returns_none() {
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("down-1", false), "down-1")
        .with_context(MockResource::new("down-2", false), "down-2")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(1)
        .build();

    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(wrapper.get_healthy().await.is_none());
    wrapper.stop().await;
}

#[tokio::test]
async fn get_status_unknown_name_returns_none() {
    let wrapper = wrapper_builder();
    assert_eq!(wrapper.get_status("nonexistent").await, None);
}

#[tokio::test]
async fn health_details_include_counters() {
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("ok", true), "ok")
        .with_context(MockResource::new("fail", false), "fail")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .build();

    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    let details = wrapper.get_health_details().await;
    assert_eq!(details.len(), 2);

    let ok_detail = details.iter().find(|d| d.name == "ok").unwrap();
    assert!(ok_detail.consecutive_successes > 0);
    assert_eq!(ok_detail.consecutive_failures, 0);

    let fail_detail = details.iter().find(|d| d.name == "fail").unwrap();
    assert!(fail_detail.consecutive_failures > 0);
    assert_eq!(fail_detail.consecutive_successes, 0);

    wrapper.stop().await;
}

#[tokio::test]
async fn round_robin_distributes_across_healthy() {
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("a", true), "a")
        .with_context(MockResource::new("b", true), "b")
        .with_context(MockResource::new("c", true), "c")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut names = vec![];
    for _ in 0..6 {
        let resource = wrapper.get_healthy().await.unwrap();
        names.push(resource.name.clone());
    }

    // Round robin should cycle through all 3
    assert_eq!(names[0], names[3]);
    assert_eq!(names[1], names[4]);
    assert_eq!(names[2], names[5]);
    // All three should appear
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), 3);

    wrapper.stop().await;
}

#[tokio::test]
async fn stop_and_restart_works() {
    let wrapper = wrapper_builder();
    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(wrapper.get_healthy().await.is_some());

    wrapper.stop().await;

    // Restart
    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(wrapper.get_healthy().await.is_some());

    wrapper.stop().await;
}

#[tokio::test]
async fn get_usable_includes_degraded() {
    struct DegradedChecker;

    impl HealthChecker<MockResource> for DegradedChecker {
        async fn check(&self, resource: &MockResource) -> HealthStatus {
            if resource.healthy {
                HealthStatus::Degraded
            } else {
                HealthStatus::Unhealthy
            }
        }
    }

    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("degraded", true), "degraded")
        .with_checker(DegradedChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .build();

    wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // get_healthy should return None (degraded != healthy)
    assert!(wrapper.get_healthy().await.is_none());

    // get_usable should return the degraded resource
    let resource = wrapper.get_usable().await;
    assert!(resource.is_some());
    assert_eq!(resource.unwrap().name, "degraded");

    wrapper.stop().await;
}

#[tokio::test]
async fn failure_threshold_delays_status_change() {
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockResource::new("failing", false), "failing")
        .with_checker(MockChecker)
        .with_interval(Duration::from_millis(50))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(3)
        .build();

    wrapper.start().await;

    // After one check cycle, threshold not yet reached
    tokio::time::sleep(Duration::from_millis(80)).await;
    // Status may still be Unknown since threshold requires 3 failures
    let status = wrapper.get_status("failing").await.unwrap();
    assert_ne!(status, HealthStatus::Healthy);

    // After enough cycles, should be Unhealthy
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = wrapper.get_status("failing").await.unwrap();
    assert_eq!(status, HealthStatus::Unhealthy);

    wrapper.stop().await;
}
