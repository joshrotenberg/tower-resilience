//! Resilient Key-Value Store with Chaos Engineering
//!
//! This example demonstrates:
//! 1. Circuit breaker with health check integration (PR #121)
//! 2. Chaos engineering with configurable failure injection
//! 3. Kubernetes-ready health endpoints
//!
//! The circuit breaker automatically opens when chaos failures exceed the threshold,
//! demonstrating real resilience patterns in action.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use rand::Rng;
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tower::{Service, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreaker, CircuitBreakerLayer};

/// Database request
#[derive(Clone, Debug)]
struct DbRequest {
    key: String,
}

/// Database response
#[derive(Clone, Debug)]
enum DbResponse {
    Found(Bytes),
    NotFound,
}

/// Database error
#[derive(Clone, Debug)]
struct DbError(String);

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Database error: {}", self.0)
    }
}

impl std::error::Error for DbError {}

// Simple wrapper service that's cloneable
#[derive(Clone)]
struct DatabaseService {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
    chaos_failure_rate: Arc<AtomicU32>,
}

impl tower::Service<DbRequest> for DatabaseService {
    type Response = DbResponse;
    type Error = DbError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: DbRequest) -> Self::Future {
        let db = Arc::clone(&self.db);
        let chaos_rate = Arc::clone(&self.chaos_failure_rate);

        Box::pin(async move {
            // Chaos injection: random failures based on configured rate
            let rate_bits = chaos_rate.load(Ordering::Relaxed);
            let failure_rate = f64::from_bits(rate_bits as u64);

            if rand::rng().random::<f64>() < failure_rate {
                tracing::warn!("Chaos: Injected database failure for key '{}'", req.key);
                return Err(DbError("Simulated database failure (chaos)".to_string()));
            }

            // Normal database operation
            let db = db.read().unwrap();
            match db.get(&req.key) {
                Some(value) => Ok(DbResponse::Found(value.clone())),
                None => Ok(DbResponse::NotFound),
            }
        })
    }
}

type DbService = CircuitBreaker<DatabaseService, DbRequest, DbResponse, DbError>;

#[derive(Clone)]
struct AppState {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
    db_service: Arc<tokio::sync::Mutex<DbService>>,
    chaos_failure_rate: Arc<AtomicU32>,
}

impl AppState {
    fn new() -> Self {
        let db: Arc<RwLock<HashMap<String, Bytes>>> = Arc::new(RwLock::new(HashMap::new()));
        let chaos_failure_rate = Arc::new(AtomicU32::new(0));

        // Create the database service
        let base_service = DatabaseService {
            db: Arc::clone(&db),
            chaos_failure_rate: Arc::clone(&chaos_failure_rate),
        };

        // Wrap with circuit breaker
        let circuit_breaker_layer = CircuitBreakerLayer::builder()
            .name("kv-store-db")
            .failure_rate_threshold(0.5)
            .sliding_window_size(10)
            .minimum_number_of_calls(5)
            .wait_duration_in_open(Duration::from_secs(5))
            .on_state_transition(|from, to| {
                tracing::info!("Circuit breaker: {:?} -> {:?}", from, to);
            })
            .on_call_rejected(|| {
                tracing::warn!("Circuit breaker rejected call (circuit OPEN)");
            })
            .build();

        let db_service = circuit_breaker_layer.layer(base_service);

        Self {
            db,
            db_service: Arc::new(tokio::sync::Mutex::new(db_service)),
            chaos_failure_rate,
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.expect("bind error");

    tracing::info!("Listening on http://{}", addr);
    tracing::info!("Health endpoints:");
    tracing::info!("  - Readiness: http://{}/health/ready", addr);
    tracing::info!("  - Liveness:  http://{}/health/live", addr);
    tracing::info!("Chaos control:");
    tracing::info!(
        "  - Set failure rate: POST http://{}/admin/chaos?rate=0.8",
        addr
    );
    tracing::info!("");
    tracing::info!("Try it:");
    tracing::info!("  curl -X POST http://{}/mykey -d 'hello world'", addr);
    tracing::info!("  curl http://{}/mykey", addr);
    tracing::info!("  curl -X POST 'http://{}/admin/chaos?rate=0.8'", addr);
    tracing::info!("  curl http://{}/metrics", addr);

    axum::serve(listener, app().into_make_service())
        .await
        .expect("server error");
}

fn app() -> Router {
    let state = AppState::new();

    Router::new()
        .route("/:key", get(get_key).post(set_key))
        .route("/health/ready", get(health_ready))
        .route("/health/live", get(health_live))
        .route("/metrics", get(get_metrics))
        .route("/admin/chaos", post(set_chaos_rate))
        .with_state(state)
}

/// Get a value from the store (goes through circuit breaker with chaos)
async fn get_key(Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    let mut svc = state.db_service.lock().await;

    // Call ready() and then call() on the service
    match ServiceExt::<DbRequest>::ready(&mut *svc).await {
        Ok(ready_svc) => match ready_svc.call(DbRequest { key: key.clone() }).await {
            Ok(DbResponse::Found(value)) => (StatusCode::OK, value).into_response(),
            Ok(DbResponse::NotFound) => {
                (StatusCode::NOT_FOUND, "Key not found".to_string()).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Service unavailable: {}", e),
        )
            .into_response(),
    }
}

/// Set a value in the store (bypasses circuit breaker - writes are assumed fast/reliable)
async fn set_key(
    Path(key): Path<String>,
    State(state): State<AppState>,
    value: Bytes,
) -> impl IntoResponse {
    let mut db = state.db.write().unwrap();
    db.insert(key, value);
    StatusCode::OK
}

/// Readiness probe - demonstrates http_status() helper from PR #121
///
/// Returns 200 when circuit is closed/half-open, 503 when open.
/// Perfect for Kubernetes readiness probes.
async fn health_ready(State(state): State<AppState>) -> impl IntoResponse {
    let svc = state.db_service.lock().await;

    // Use the http_status() helper method from PR #121
    let status = svc.http_status();

    // Use the health_status() helper method from PR #121
    let health = svc.health_status();

    let circuit_state = svc.state_sync();

    (
        StatusCode::from_u16(status).unwrap(),
        Json(serde_json::json!({
            "status": health,
            "circuit_state": format!("{:?}", circuit_state),
            "http_status": status,
            "message": match status {
                200 => "Service is ready to accept traffic",
                _ => "Service is degraded - circuit breaker is open"
            }
        })),
    )
}

/// Liveness probe - always returns 200 (app is alive)
async fn health_live() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "alive"
        })),
    )
}

/// Metrics endpoint - shows circuit breaker state and chaos configuration
#[axum::debug_handler]
async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    // Collect all data while holding the lock, then drop it
    let (metrics, health, circuit_state, http_status) = {
        let svc = state.db_service.lock().await;
        let metrics = svc.metrics().await;
        let health = svc.health_status();
        let circuit_state = format!("{:?}", svc.state_sync());
        let http_status = svc.http_status();
        (metrics, health, circuit_state, http_status)
    }; // MutexGuard dropped here

    let rate_bits = state.chaos_failure_rate.load(Ordering::Relaxed);
    let chaos_rate = f64::from_bits(rate_bits as u64);
    let keys_count = state.db.read().unwrap().len();

    Json(serde_json::json!({
        "circuit_breaker": {
            "health": health,
            "state": circuit_state,
            "http_status": http_status,
            "metrics": {
                "success_count": metrics.success_count,
                "failure_count": metrics.failure_count,
                "total_calls": metrics.total_calls,
                "failure_rate": metrics.failure_rate,
            }
        },
        "chaos": {
            "failure_rate": chaos_rate,
            "failure_rate_percent": chaos_rate * 100.0,
        },
        "database": {
            "keys_stored": keys_count,
        }
    }))
}

#[derive(Deserialize)]
struct ChaosParams {
    rate: f64,
}

/// Admin endpoint to configure chaos failure rate
///
/// Examples:
/// - rate=0.0 → No failures (circuit stays closed)
/// - rate=0.5 → 50% failures (may trip circuit)
/// - rate=0.9 → 90% failures (will definitely trip circuit)
async fn set_chaos_rate(
    State(state): State<AppState>,
    Query(params): Query<ChaosParams>,
) -> impl IntoResponse {
    let rate = params.rate.clamp(0.0, 1.0);
    state
        .chaos_failure_rate
        .store(rate.to_bits() as u32, Ordering::Relaxed);

    tracing::info!("Chaos failure rate set to {:.1}%", rate * 100.0);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": format!("Chaos failure rate set to {:.1}%", rate * 100.0),
            "tip": "Make several GET requests to see the circuit breaker respond to failures"
        })),
    )
}
