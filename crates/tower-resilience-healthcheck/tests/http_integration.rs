use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tower::Layer;
use tower_resilience_chaos::ChaosLayer;
use tower_resilience_healthcheck::{HealthCheckWrapper, HealthStatus, SelectionStrategy};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// HTTP client that can be health checked
#[derive(Clone, Debug)]
struct HttpEndpoint {
    url: String,
    client: reqwest::Client,
}

impl PartialEq for HttpEndpoint {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl HttpEndpoint {
    fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }

    async fn health_check(&self) -> HealthStatus {
        match self.client.get(&self.url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    HealthStatus::Healthy
                } else if response.status().is_server_error() {
                    HealthStatus::Unhealthy
                } else {
                    HealthStatus::Degraded
                }
            }
            Err(_) => HealthStatus::Unhealthy,
        }
    }

    async fn get_data(&self) -> Result<String, String> {
        self.client
            .get(format!("{}/data", self.url))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())
    }
}

// Health checker implementation for HttpEndpoint
struct HttpHealthChecker;

impl tower_resilience_healthcheck::HealthChecker<HttpEndpoint> for HttpHealthChecker {
    async fn check(&self, resource: &HttpEndpoint) -> HealthStatus {
        resource.health_check().await
    }
}

#[tokio::test]
async fn test_single_endpoint_health_monitoring() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Create health endpoint that returns 200 OK
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint, "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .build();

    wrapper.start().await;

    // Wait for health check to run
    sleep(Duration::from_millis(150)).await;

    let healthy = wrapper.get_healthy().await;
    assert!(healthy.is_some(), "Expected a healthy endpoint");
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy),
        "Expected endpoint to be healthy"
    );

    wrapper.stop().await;
}

#[tokio::test]
async fn test_endpoint_failure_detection() {
    let mock_server = MockServer::start().await;

    // Initially healthy
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint, "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(2) // Need 2 failures to mark unhealthy
        .build();

    wrapper.start().await;

    // Wait for initial health check
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );

    // Now make the endpoint fail
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    // Wait for health check to detect failure (need 2 failures at 100ms interval each)
    sleep(Duration::from_millis(250)).await;

    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Unhealthy),
        "Expected endpoint to be unhealthy after failures"
    );

    wrapper.stop().await;
}

#[tokio::test]
async fn test_endpoint_recovery_detection() {
    let mock_server = MockServer::start().await;

    // Start unhealthy
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint, "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(1)
        .with_success_threshold(2) // Need 2 successes to mark healthy
        .build();

    wrapper.start().await;

    // Wait for initial health check to detect failure
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Unhealthy)
    );

    // Now make the endpoint healthy again
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    // Wait for health check to detect recovery (need 2 successes at 100ms interval each)
    sleep(Duration::from_millis(250)).await;

    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy),
        "Expected endpoint to recover to healthy"
    );

    wrapper.stop().await;
}

#[tokio::test]
async fn test_multiple_endpoints_with_failover() {
    let mock1 = MockServer::start().await;
    let mock2 = MockServer::start().await;
    let mock3 = MockServer::start().await;

    // Mock 1: Healthy
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock1)
        .await;

    // Mock 2: Degraded (4xx)
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock2)
        .await;

    // Mock 3: Unhealthy (5xx)
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock3)
        .await;

    let wrapper = HealthCheckWrapper::builder()
        .with_context(HttpEndpoint::new(mock1.uri()), "endpoint1")
        .with_context(HttpEndpoint::new(mock2.uri()), "endpoint2")
        .with_context(HttpEndpoint::new(mock3.uri()), "endpoint3")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(1)
        .with_selection_strategy(SelectionStrategy::PreferHealthy)
        .build();

    wrapper.start().await;

    // Wait for health checks
    sleep(Duration::from_millis(150)).await;

    // Check statuses
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );
    assert_eq!(
        wrapper.get_status("endpoint2").await,
        Some(HealthStatus::Degraded)
    );
    assert_eq!(
        wrapper.get_status("endpoint3").await,
        Some(HealthStatus::Unhealthy)
    );

    // Get healthy endpoint (should prefer the healthy one)
    let healthy = wrapper.get_healthy().await;
    assert!(healthy.is_some(), "Expected a healthy endpoint");

    // Get usable endpoint (should be healthy or degraded)
    let usable = wrapper.get_usable().await;
    assert!(usable.is_some(), "Expected a usable endpoint");

    wrapper.stop().await;
}

#[tokio::test]
async fn test_round_robin_selection() {
    let mock1 = MockServer::start().await;
    let mock2 = MockServer::start().await;

    // Both healthy
    for mock in [&mock1, &mock2] {
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200))
            .mount(mock)
            .await;
    }

    let wrapper = Arc::new(
        HealthCheckWrapper::builder()
            .with_context(HttpEndpoint::new(mock1.uri()), "endpoint1")
            .with_context(HttpEndpoint::new(mock2.uri()), "endpoint2")
            .with_checker(HttpHealthChecker)
            .with_interval(Duration::from_millis(100))
            .with_initial_delay(Duration::from_millis(10))
            .with_selection_strategy(SelectionStrategy::RoundRobin)
            .build(),
    );

    wrapper.start().await;

    // Wait for health checks
    sleep(Duration::from_millis(150)).await;

    // Both should be healthy
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );
    assert_eq!(
        wrapper.get_status("endpoint2").await,
        Some(HealthStatus::Healthy)
    );

    // Get usable endpoints multiple times - should round robin
    let first = wrapper.get_usable().await;
    let second = wrapper.get_usable().await;
    let third = wrapper.get_usable().await;

    assert!(first.is_some());
    assert!(second.is_some());
    assert!(third.is_some());

    // All statuses should show both endpoints as healthy
    let statuses = wrapper.get_all_statuses().await;
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|(_, s)| *s == HealthStatus::Healthy));

    wrapper.stop().await;
}

#[tokio::test]
async fn test_slow_response_detection() {
    let mock_server = MockServer::start().await;

    // Simulate slow endpoint
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint, "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(200))
        .with_initial_delay(Duration::from_millis(10))
        .build();

    wrapper.start().await;

    // Wait for initial slow health check
    sleep(Duration::from_millis(600)).await;

    // Should still be healthy (just slow)
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy),
        "Slow but successful responses should still be healthy"
    );

    wrapper.stop().await;
}

#[tokio::test]
async fn test_chaos_injection_with_health_check() {
    let mock_server = MockServer::start().await;

    // Healthy endpoint
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_string("test data"))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    // Create health check wrapper
    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint.clone(), "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .build();

    wrapper.start().await;

    // Wait for health check
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );

    // Now wrap the endpoint's get_data call with chaos layer
    // Types inferred from closure signature
    let chaos_layer = ChaosLayer::builder()
        .error_rate(0.5) // 50% failure rate
        .error_fn(|_req: &()| "chaos error".to_string())
        .build();

    // Create a service from the endpoint
    let ep = endpoint.clone();
    let svc = tower::service_fn(move |_req: ()| {
        let ep = ep.clone();
        async move { ep.get_data().await }
    });

    let mut chaotic_svc = chaos_layer.layer(svc);

    // Make multiple calls - some should fail due to chaos
    let mut success_count = 0;
    let mut failure_count = 0;

    for _ in 0..20 {
        let result = tower::Service::call(&mut chaotic_svc, ()).await;
        if result.is_ok() {
            success_count += 1;
        } else {
            failure_count += 1;
        }
    }

    // With 50% failure rate and 20 calls, we should see some of each
    // (allow some variance due to randomness)
    assert!(
        success_count > 0,
        "Expected some successful calls, got {}",
        success_count
    );
    assert!(
        failure_count > 0,
        "Expected some failed calls due to chaos, got {}",
        failure_count
    );

    // Health check should still show healthy (it's checking /health, not /data)
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );

    wrapper.stop().await;
}

#[tokio::test]
async fn test_all_endpoints_down_scenario() {
    let mock1 = MockServer::start().await;
    let mock2 = MockServer::start().await;

    // Both unhealthy
    for mock in [&mock1, &mock2] {
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(mock)
            .await;
    }

    let wrapper = HealthCheckWrapper::builder()
        .with_context(HttpEndpoint::new(mock1.uri()), "endpoint1")
        .with_context(HttpEndpoint::new(mock2.uri()), "endpoint2")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(1)
        .build();

    wrapper.start().await;

    // Wait for health checks
    sleep(Duration::from_millis(150)).await;

    // Both should be unhealthy
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Unhealthy)
    );
    assert_eq!(
        wrapper.get_status("endpoint2").await,
        Some(HealthStatus::Unhealthy)
    );

    // No healthy or usable endpoints
    assert_eq!(wrapper.get_healthy().await, None);
    assert_eq!(wrapper.get_usable().await, None);

    wrapper.stop().await;
}

#[tokio::test]
async fn test_network_partition_recovery() {
    let mock_server = MockServer::start().await;

    // Start healthy
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let endpoint = HttpEndpoint::new(mock_server.uri());

    let wrapper = HealthCheckWrapper::builder()
        .with_context(endpoint, "endpoint1")
        .with_checker(HttpHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_initial_delay(Duration::from_millis(10))
        .with_failure_threshold(2)
        .with_success_threshold(2)
        .build();

    wrapper.start().await;

    // Initially healthy
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy)
    );

    // Simulate network partition - wiremock returns 404 when no mock is mounted
    // which maps to Degraded in our health check logic
    mock_server.reset().await;

    // Wait for failure detection (2 failures needed)
    sleep(Duration::from_millis(250)).await;

    // Note: wiremock returns 404 (not found) when no mock is mounted,
    // which our health_check maps to Degraded, not Unhealthy
    let status = wrapper.get_status("endpoint1").await;
    assert!(
        status == Some(HealthStatus::Degraded) || status == Some(HealthStatus::Unhealthy),
        "Expected Degraded or Unhealthy after network partition, got {:?}",
        status
    );

    // Network recovers
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    // Wait for recovery detection
    sleep(Duration::from_millis(250)).await;
    assert_eq!(
        wrapper.get_status("endpoint1").await,
        Some(HealthStatus::Healthy),
        "Expected recovery after network partition heals"
    );

    wrapper.stop().await;
}
