//! Simple Resilient Key-Value Store
//!
//! This example demonstrates the new `http_status()` and `health_status()` helper methods
//! from PR #121 for implementing health check endpoints.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::net::TcpListener;
use tower_resilience_circuitbreaker::{CircuitBreaker, CircuitBreakerLayer};

type AppCircuitBreaker = CircuitBreaker<tower::util::BoxService<(), (), String>, (), (), String>;

#[derive(Clone)]
struct AppState {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
    circuit_breaker: Arc<tokio::sync::Mutex<AppCircuitBreaker>>,
}

impl AppState {
    fn new() -> Self {
        let dummy_service = tower::service_fn(|_: ()| async { Ok::<(), String>(()) });

        let circuit_breaker_layer = CircuitBreakerLayer::builder()
            .name("kv-store")
            .failure_rate_threshold(0.5)
            .sliding_window_size(10)
            .minimum_number_of_calls(5)
            .wait_duration_in_open(Duration::from_secs(5))
            .on_state_transition(|from, to| {
                tracing::info!("Circuit breaker: {:?} -> {:?}", from, to);
            })
            .build();

        let circuit_breaker =
            circuit_breaker_layer.layer(tower::util::BoxService::new(dummy_service));

        Self {
            db: Arc::new(RwLock::new(HashMap::new())),
            circuit_breaker: Arc::new(tokio::sync::Mutex::new(circuit_breaker)),
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
        .route("/admin/circuit/open", post(force_open_circuit))
        .route("/admin/circuit/close", post(force_close_circuit))
        .with_state(state)
}

async fn get_key(Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.read().unwrap();
    match db.get(&key) {
        Some(value) => (StatusCode::OK, value.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "Key not found").into_response(),
    }
}

async fn set_key(
    Path(key): Path<String>,
    State(state): State<AppState>,
    value: Bytes,
) -> impl IntoResponse {
    let mut db = state.db.write().unwrap();
    db.insert(key, value);
    StatusCode::OK
}

async fn health_ready(State(state): State<AppState>) -> impl IntoResponse {
    let breaker = state.circuit_breaker.lock().await;

    let status = breaker.http_status();
    let health = breaker.health_status();
    let circuit_state = breaker.state_sync();

    (
        StatusCode::from_u16(status).unwrap(),
        Json(serde_json::json!({
            "status": health,
            "circuit_state": format!("{:?}", circuit_state),
            "http_status": status,
        })),
    )
}

async fn health_live() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "alive"
        })),
    )
}

async fn force_open_circuit(State(state): State<AppState>) -> impl IntoResponse {
    let breaker = state.circuit_breaker.lock().await;
    breaker.force_open().await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Circuit breaker opened"
        })),
    )
}

async fn force_close_circuit(State(state): State<AppState>) -> impl IntoResponse {
    let breaker = state.circuit_breaker.lock().await;
    breaker.force_closed().await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Circuit breaker closed"
        })),
    )
}
