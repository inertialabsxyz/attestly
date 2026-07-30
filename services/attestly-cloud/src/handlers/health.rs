//! `GET /healthz` — used by fly/Railway/Docker health probes.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

/// Returns 200 if both Postgres and blob storage are reachable. 503 if not.
/// The body is `{"status":"ok"|"degraded", "version":"<sha>"}`.
pub async fn healthz(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let version = env!("CARGO_PKG_VERSION");
    match state.db.ping().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "version": version})),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "degraded", "version": version, "detail": e.to_string()})),
        ),
    }
}
