//! `GET /healthz` — used by fly/Railway/Docker health probes.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

/// Returns 200 if both Postgres and blob storage are reachable. 503 if not.
/// The body is `{"status":"ok"|"degraded", "version":"<sha>"}`, plus a
/// `detail` on the degraded path.
///
/// Both probes are read-only and cheap — this endpoint is hit every 10s by the
/// Fly healthcheck. The blob probe exists because a missing volume mount is
/// otherwise invisible: `FsBlobStore` would happily write into the container's
/// ephemeral layer and lose every receipt on the next machine restart.
pub async fn healthz(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let version = env!("CARGO_PKG_VERSION");
    let degraded = |detail: String| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "degraded", "version": version, "detail": detail})),
        )
    };
    if let Err(e) = state.db.ping().await {
        return degraded(e.to_string());
    }
    if let Err(e) = state.blob.health().await {
        return degraded(e.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "version": version})),
    )
}
