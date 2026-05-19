//! Share-link routes — scoped, time-limited, revocable tokens that grant
//! public read access to one or more attestations.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::Json;
use base64::Engine as _;
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use awp_core::Attestation;

use crate::auth::AuthedAccount;
use crate::error::{ApiError, ApiResult};
use crate::handlers::attestations::fetch_and_verify;
use crate::state::AppState;
use crate::store::{SearchFilters, ShareLink};

const DEFAULT_EXPIRES_IN_DAYS: i64 = 30;
const MAX_EXPIRES_IN_DAYS: i64 = 365;
const TOKEN_BYTES: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShareLinkRequest {
    pub attestation_ids: Option<Vec<Uuid>>,
    pub filter: Option<FilterShape>,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterShape {
    pub agent_id: Option<String>,
    pub customer_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize)]
pub struct CreateShareLinkResponse {
    pub token: String,
    pub url: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// `POST /v1/share-links`. Either `attestation_ids` or `filter` must be
/// supplied, not both.
pub async fn create(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Json(req): Json<CreateShareLinkRequest>,
) -> ApiResult<impl IntoResponse> {
    let expires_in = req.expires_in_days.unwrap_or(DEFAULT_EXPIRES_IN_DAYS);
    if !(1..=MAX_EXPIRES_IN_DAYS).contains(&expires_in) {
        return Err(ApiError::InvalidRequest(format!(
            "expires_in_days must be between 1 and {MAX_EXPIRES_IN_DAYS}"
        )));
    }

    let ids: Vec<Uuid> = match (req.attestation_ids, req.filter) {
        (Some(ids), None) => {
            if ids.is_empty() {
                return Err(ApiError::InvalidRequest(
                    "attestation_ids must be non-empty".into(),
                ));
            }
            // Every id must belong to the authenticated account; we refuse
            // to share what we don't own.
            for id in &ids {
                if state.db.fetch_attestation(account.id, *id).await?.is_none() {
                    return Err(ApiError::NotFound);
                }
            }
            ids
        }
        (None, Some(filter)) => {
            // Materialise the filter into a concrete id list at link-creation
            // time. This means the link is stable: it points to the exact
            // attestations that matched today, not whatever happens to match
            // when someone clicks the link next week.
            let filters = SearchFilters {
                agent_id: filter.agent_id,
                customer_id: filter.customer_id,
                from: filter.from.as_deref().map(parse_ts).transpose()?,
                to: filter.to.as_deref().map(parse_ts).transpose()?,
                limit: 500,
                cursor: None,
            };
            let page = state.db.search_attestations(account.id, filters).await?;
            if page.rows.is_empty() {
                return Err(ApiError::InvalidRequest(
                    "filter matched no attestations".into(),
                ));
            }
            page.rows.into_iter().map(|r| r.id).collect()
        }
        (Some(_), Some(_)) => {
            return Err(ApiError::InvalidRequest(
                "exactly one of attestation_ids or filter is required, not both".into(),
            ))
        }
        (None, None) => {
            return Err(ApiError::InvalidRequest(
                "exactly one of attestation_ids or filter is required".into(),
            ))
        }
    };

    let token = generate_token();
    let now = Utc::now();
    let link = ShareLink {
        token: token.clone(),
        account_id: account.id,
        attestation_ids: ids,
        expires_at: now + Duration::days(expires_in),
        created_at: now,
        revoked_at: None,
    };
    state.db.create_share_link(link.clone()).await?;
    let url = public_url_for(&token);
    Ok((
        StatusCode::CREATED,
        Json(CreateShareLinkResponse {
            token,
            url,
            expires_at: link.expires_at,
        }),
    ))
}

fn parse_ts(s: &str) -> Result<chrono::DateTime<Utc>, ApiError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| ApiError::InvalidRequest(format!("invalid timestamp: {e}")))
}

/// URL-safe 32-byte token, base64-no-padding encoded → 43 chars.
fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn public_url_for(token: &str) -> String {
    // The placeholder app domain — the real domain is wired by the operator
    // at deploy time via `AWP_CLOUD_BASE_URL`. Tests don't care.
    let base = std::env::var("AWP_CLOUD_BASE_URL")
        .unwrap_or_else(|_| "https://app.awp-cloud.xyz".to_string());
    format!("{base}/share/{token}")
}

#[derive(Serialize)]
pub struct PublicShareResponse {
    pub expires_at: chrono::DateTime<Utc>,
    pub attestations: Vec<Attestation>,
}

/// `GET /v1/share-links/{token}` — public, machine-readable. Pipes through
/// `fetch_and_verify` for each attestation, so a tampered blob still
/// surfaces as 422 even via a share link.
pub async fn get_json(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Json<PublicShareResponse>> {
    let link = resolve_active_link(&state, &token).await?;
    let mut atts = Vec::with_capacity(link.attestation_ids.len());
    for id in &link.attestation_ids {
        let Some(index) = state.db.fetch_attestation(link.account_id, *id).await? else {
            // Soft-skip: the link was created with this id, but the underlying
            // row no longer exists. Surface to the viewer rather than 500.
            continue;
        };
        let att = fetch_and_verify(&state, &index).await?;
        atts.push(att);
    }
    Ok(Json(PublicShareResponse {
        expires_at: link.expires_at,
        attestations: atts,
    }))
}

/// `GET /share/{token}` — public, server-rendered HTML.
///
/// Returns the same static-viewer HTML as the OSS audit viewer, bootstrapped
/// with the attestation payload inline so the page re-verifies in-browser
/// independently of the server. This is the "tamper-evident even against a
/// compromised server" guarantee.
pub async fn render_page(
    State(state): State<AppState>,
    Path(token): Path<String>,
    _headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    let link = match resolve_active_link(&state, &token).await {
        Ok(l) => l,
        Err(ApiError::LinkNotFound) | Err(ApiError::NotFound) => {
            return Ok(Html(crate::web::not_found_html()));
        }
        Err(e) => return Err(e),
    };
    let mut atts = Vec::new();
    for id in &link.attestation_ids {
        if let Some(index) = state.db.fetch_attestation(link.account_id, *id).await? {
            match fetch_and_verify(&state, &index).await {
                Ok(a) => atts.push(a),
                Err(ApiError::SignatureInvalid(detail)) => {
                    // Render the page anyway with a banner so the auditor sees
                    // tamper, instead of an opaque 422.
                    let html = crate::web::tampered_html(&detail);
                    return Ok(Html(html));
                }
                Err(e) => return Err(e),
            }
        }
    }
    let html = crate::web::share_html(&token, &link.expires_at, &atts);
    Ok(Html(html))
}

/// `DELETE /v1/share-links/{token}` — owner-only revocation.
pub async fn revoke(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Path(token): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let revoked = state.db.revoke_share_link(account.id, &token).await?;
    if !revoked {
        return Err(ApiError::NotFound);
    }
    Ok((StatusCode::NO_CONTENT, [(header::CONTENT_TYPE, "")]))
}

async fn resolve_active_link(state: &AppState, token: &str) -> Result<ShareLink, ApiError> {
    let Some(link) = state.db.fetch_share_link(token).await? else {
        return Err(ApiError::LinkNotFound);
    };
    if link.revoked_at.is_some() {
        return Err(ApiError::LinkNotFound);
    }
    if Utc::now() >= link.expires_at {
        return Err(ApiError::LinkNotFound);
    }
    Ok(link)
}
