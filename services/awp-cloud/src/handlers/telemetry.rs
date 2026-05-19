//! Anonymous SDK telemetry ingest + admin conversion-ready dashboard.
//!
//! ## Privacy guarantees
//!
//! The ingest endpoint is intentionally outside the `x-api-key` gate: the
//! SDK posts before any user has signed up, and the payload contains no PII
//! and no foreign key to `accounts`. The deliberate lack of a join is the
//! privacy contract — a telemetry row never resolves back to a named user
//! unless that user separately signs up through `/v1/account/signup`.
//!
//! The schema is documented in `services/awp-cloud/migrations/0004_quickstart_telemetry.sql`.
//!
//! ## Admin dashboard
//!
//! `GET /v1/admin/telemetry/conversion-ready` returns OSS installs that
//! exceed the daily FileSink threshold in the trailing window — the
//! threshold called out in `planning/gtm-phase-2-plan.md` Step 5 as the
//! conversion-readiness signal (default: 10k attestations/day). Gated by
//! the same admin key used by the end-of-period billing trigger.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::TelemetryEvent;

const ADMIN_HEADER: &str = "x-admin-key";
const DEFAULT_CONVERSION_THRESHOLD: i64 = 10_000;
const DEFAULT_WINDOW_DAYS: i64 = 7;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct UsageReport {
    pub install_id: Uuid,
    pub sdk_version: String,
    pub attestations_emitted_today: i64,
    pub sink_type: String,
    pub python_version: String,
    pub os: String,
}

/// `POST /v1/telemetry/usage` — unauthenticated daily aggregate from the SDK.
pub async fn ingest_usage(
    State(state): State<AppState>,
    Json(report): Json<UsageReport>,
) -> ApiResult<impl IntoResponse> {
    if report.attestations_emitted_today < 0 {
        return Err(ApiError::InvalidRequest(
            "attestations_emitted_today must be non-negative".into(),
        ));
    }
    if report.sdk_version.is_empty() || report.sdk_version.len() > 64 {
        return Err(ApiError::InvalidRequest("invalid sdk_version".into()));
    }
    if report.sink_type.is_empty() || report.sink_type.len() > 64 {
        return Err(ApiError::InvalidRequest("invalid sink_type".into()));
    }

    let now = Utc::now();
    let event = TelemetryEvent {
        install_id: report.install_id,
        day: now.date_naive(),
        sdk_version: report.sdk_version,
        sink_type: report.sink_type,
        python_version: truncate(report.python_version, 32),
        os: truncate(report.os, 32),
        attestations_emitted_today: report.attestations_emitted_today,
        last_seen_at: now,
    };

    state.db.upsert_telemetry_event(event).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn truncate(s: String, n: usize) -> String {
    if s.len() <= n {
        s
    } else {
        s.chars().take(n).collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct ConversionReadyQuery {
    pub days: Option<i64>,
    pub min_attestations: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ConversionReadyResponse {
    pub from_day: chrono::NaiveDate,
    pub min_attestations: i64,
    pub installs: Vec<TelemetryEvent>,
}

/// `GET /v1/admin/telemetry/conversion-ready` — list FileSink installs above
/// the conversion-ready threshold. Admin-only.
pub async fn conversion_ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ConversionReadyQuery>,
) -> ApiResult<Json<ConversionReadyResponse>> {
    require_admin(&headers, &state)?;

    let window = q.days.unwrap_or(DEFAULT_WINDOW_DAYS).clamp(1, 90);
    let min_attestations = q
        .min_attestations
        .unwrap_or(DEFAULT_CONVERSION_THRESHOLD)
        .max(0);
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let from = Utc::now().date_naive() - Duration::days(window);
    let installs = state
        .db
        .top_telemetry_volumes(from, min_attestations, limit)
        .await?;
    Ok(Json(ConversionReadyResponse {
        from_day: from,
        min_attestations,
        installs,
    }))
}

fn require_admin(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    let supplied = headers
        .get(ADMIN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if supplied.is_empty() || supplied != state.billing.admin_key {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}
