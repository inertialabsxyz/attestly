//! `/v1/attestations` routes — the load-bearing ingest, search, and tamper-
//! detection surface. Contract spec lives in
//! `services/awp-cloud/API.md` under "Endpoints".

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use awp_core::Attestation;

use crate::auth::AuthedAccount;
use crate::canonical::{blob_sha256, canonical_blob_bytes, parse_blob, reverify};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{AttestationIndex, IngestOutcome, SearchFilters};

const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 500;

#[derive(Serialize)]
pub struct IngestResponse {
    pub id: Uuid,
    pub received_at: DateTime<Utc>,
    pub blob_sha256: String,
}

/// `POST /v1/attestations` — accept a signed attestation.
///
/// Flow:
/// 1. Parse the body into an `Attestation`.
/// 2. Re-verify the signature against the embedded `agent_pubkey` using
///    `awp-core::verify_attestation_signature`. Failure → 422.
/// 3. Compute `blob_sha256` over the canonical bytes.
/// 4. `BlobStore::put` (idempotent on the content address).
/// 5. `Db::insert_attestation` with the metadata row. Idempotent: same
///    attestation id returns the existing record (200), not a duplicate
///    insert.
/// 6. `Db::record_usage` for billing.
pub async fn ingest(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Json(att): Json<Attestation>,
) -> ApiResult<impl IntoResponse> {
    if !reverify(&att) {
        return Err(ApiError::SignatureInvalid(
            "ed25519 verification failed against embedded agent_pubkey".into(),
        ));
    }

    let canonical = canonical_blob_bytes(&att);
    let sha = blob_sha256(&canonical);
    let sha_hex = hex::encode(sha);

    state.blob.put(&sha_hex, &canonical).await?;

    let customer_id = extract_customer_id(&att.output);
    let received_at = Utc::now();
    let index = AttestationIndex {
        id: att.id,
        account_id: account.id,
        agent_id: att.agent_id.clone(),
        agent_pubkey_hex: hex::encode(att.agent_pubkey),
        customer_id,
        received_at,
        blob_sha256_hex: sha_hex.clone(),
    };

    let outcome = state.db.insert_attestation(index).await?;
    let (status, stored) = match outcome {
        IngestOutcome::Inserted(s) => {
            // Usage counter only ticks on first insert — duplicates don't
            // count toward metered billing.
            state.db.record_usage(account.id, s.received_at).await?;
            (StatusCode::CREATED, s)
        }
        IngestOutcome::Existed(s) => (StatusCode::OK, s),
    };

    let body = IngestResponse {
        id: stored.id,
        received_at: stored.received_at,
        blob_sha256: stored.blob_sha256_hex,
    };
    Ok((status, Json(body)))
}

/// Look at the attestation's `output` payload and pull out
/// `output.customer_id` if it's a JSON string. KYC attestations encode it
/// this way; non-KYC attestations don't, and we leave `customer_id` null.
fn extract_customer_id(output: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(output).ok()?;
    parsed.get("customer_id")?.as_str().map(|s| s.to_string())
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub agent_id: Option<String>,
    pub customer_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct SearchResponseRow {
    pub id: Uuid,
    pub agent_id: String,
    pub agent_pubkey: String,
    pub customer_id: Option<String>,
    pub received_at: DateTime<Utc>,
    pub blob_sha256: String,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub attestations: Vec<SearchResponseRow>,
    pub next_cursor: Option<String>,
}

/// `GET /v1/attestations` — paginated metadata search.
pub async fn search(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<SearchResponse>> {
    let limit = q.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if limit == 0 || limit > MAX_SEARCH_LIMIT {
        return Err(ApiError::InvalidRequest(format!(
            "limit must be between 1 and {MAX_SEARCH_LIMIT}"
        )));
    }
    let from = parse_optional_ts(q.from.as_deref(), "from")?;
    let to = parse_optional_ts(q.to.as_deref(), "to")?;

    let filters = SearchFilters {
        agent_id: q.agent_id,
        customer_id: q.customer_id,
        from,
        to,
        limit,
        cursor: q.cursor,
    };
    let page = state.db.search_attestations(account.id, filters).await?;
    Ok(Json(SearchResponse {
        attestations: page
            .rows
            .into_iter()
            .map(|r| SearchResponseRow {
                id: r.id,
                agent_id: r.agent_id,
                agent_pubkey: r.agent_pubkey_hex,
                customer_id: r.customer_id,
                received_at: r.received_at,
                blob_sha256: r.blob_sha256_hex,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

fn parse_optional_ts(raw: Option<&str>, name: &str) -> Result<Option<DateTime<Utc>>, ApiError> {
    match raw {
        None => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|e| ApiError::InvalidRequest(format!("invalid {name}: {e}"))),
    }
}

/// `GET /v1/attestations/{id}` — fetch one attestation by id. Tamper
/// detection runs unconditionally on the read path.
pub async fn get_one(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Attestation>> {
    let Some(index) = state.db.fetch_attestation(account.id, id).await? else {
        return Err(ApiError::NotFound);
    };

    let att = fetch_and_verify(&state, &index).await?;
    Ok(Json(att))
}

/// Shared "load from blob storage and re-verify" path. Used by:
/// - `GET /v1/attestations/{id}`
/// - `GET /v1/share-links/{token}` and `GET /share/{token}`
/// - `GET /v1/export`
///
/// **This is the load-bearing tamper guarantee.** Any code path that returns
/// attestation bytes to a client must go through here. If `reverify` fails,
/// we surface as 422 — never silently passing through a tampered record.
pub async fn fetch_and_verify(
    state: &AppState,
    index: &AttestationIndex,
) -> Result<Attestation, ApiError> {
    let Some(bytes) = state.blob.get(&index.blob_sha256_hex).await? else {
        return Err(ApiError::SignatureInvalid(format!(
            "stored attestation failed re-verification — blob {} missing",
            index.blob_sha256_hex
        )));
    };

    // Defence in depth: the content address says these bytes hash to
    // `blob_sha256_hex`. If they don't, something corrupted at rest.
    let actual_sha = hex::encode(blob_sha256(&bytes));
    if actual_sha != index.blob_sha256_hex {
        return Err(ApiError::SignatureInvalid(
            "stored attestation failed re-verification — content address mismatch (possible tamper at rest)"
                .into(),
        ));
    }

    let Some(att) = parse_blob(&bytes) else {
        return Err(ApiError::SignatureInvalid(
            "stored attestation failed re-verification — bytes do not parse as Attestation".into(),
        ));
    };
    if !reverify(&att) {
        return Err(ApiError::SignatureInvalid(
            "stored attestation failed re-verification — possible tamper at rest".into(),
        ));
    }
    Ok(att)
}
