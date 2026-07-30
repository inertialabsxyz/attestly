//! Account dashboard handlers. Surfaces:
//!
//! - `GET    /v1/account`              — plan + Stripe linkage + retention
//! - `GET    /v1/account/usage`        — last 30 days of daily attestation counts
//! - `GET    /v1/account/api-keys`     — active key list (metadata only — never cleartext)
//! - `POST   /v1/account/api-keys`     — create a new key; cleartext returned exactly once
//! - `DELETE /v1/account/api-keys/:id` — revoke a key
//! - `GET    /dashboard`               — server-rendered HTML dashboard
//!
//! All `/v1/account/*` routes are gated by the same `x-api-key` extractor
//! as the rest of the surface. The HTML dashboard at `/dashboard` is a
//! lightweight progressive page — the API key is supplied via the
//! `x-api-key` header by a JS bootstrap; copy/pasted by the user from the
//! welcome banner.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{generate_api_key, hash_api_key, AuthedAccount};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{DailyUsage, Plan};

#[derive(Debug, Serialize)]
pub struct AccountSummary {
    pub id: Uuid,
    pub email: String,
    pub plan: Plan,
    pub retention_days: i32,
    pub stripe_customer_id: Option<String>,
    pub has_stripe_subscription: bool,
    pub usage_this_period: i64,
    pub included_attestations: i64,
}

pub async fn get(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
) -> ApiResult<Json<AccountSummary>> {
    let usage_this_period = state.db.account_usage_total(account.id).await?;
    Ok(Json(AccountSummary {
        id: account.id,
        email: account.email.clone(),
        plan: account.plan,
        retention_days: account.retention_days,
        stripe_customer_id: account.stripe_customer_id.clone(),
        has_stripe_subscription: account.stripe_subscription_id.is_some(),
        usage_this_period,
        included_attestations: account.plan.included_attestations(),
    }))
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub points: Vec<DailyUsage>,
}

pub async fn usage(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
) -> ApiResult<Json<UsageResponse>> {
    let now = Utc::now();
    let to = now.date_naive() + Duration::days(1);
    let from = to - Duration::days(30);
    let points = state.db.usage_for_period(account.id, from, to).await?;
    Ok(Json(UsageResponse { points }))
}

#[derive(Debug, Serialize)]
pub struct ApiKeySummary {
    pub id: Uuid,
    pub project_name: String,
    pub created_at: chrono::DateTime<Utc>,
}

pub async fn list_keys(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
) -> ApiResult<Json<Vec<ApiKeySummary>>> {
    let rows = state.db.list_active_api_keys(account.id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|k| ApiKeySummary {
                id: k.id,
                project_name: k.project_name,
                created_at: k.created_at,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub project_name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateKeyResponse {
    pub id: Uuid,
    pub project_name: String,
    /// The cleartext key — surfaced exactly once at creation time. Clients
    /// must capture it; we cannot recover it from the database afterward.
    pub key: String,
}

pub async fn create_key(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Json(req): Json<CreateKeyRequest>,
) -> ApiResult<(StatusCode, Json<CreateKeyResponse>)> {
    if req.project_name.trim().is_empty() {
        return Err(ApiError::InvalidRequest("project_name is required".into()));
    }
    let cleartext = generate_api_key();
    let phc = hash_api_key(&cleartext)?;
    let rec = state
        .db
        .create_api_key(account.id, &req.project_name, &phc)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            id: rec.id,
            project_name: rec.project_name,
            key: cleartext,
        }),
    ))
}

pub async fn revoke_key(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let ok = state.db.revoke_api_key(account.id, id).await?;
    if !ok {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /dashboard` — server-rendered HTML shell. JS in the page reads the
/// API key from `localStorage` (the welcome banner writes it once after
/// signup) and pulls in the JSON endpoints above.
pub async fn render_dashboard() -> Html<String> {
    Html(crate::web::dashboard_html())
}
