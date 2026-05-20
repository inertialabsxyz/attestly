//! `GET /v1/export` — stream every attestation owned by the caller as JSON
//! Lines (one canonical `Attestation` per line). Backs the "free to leave"
//! guarantee on the pricing page (Step 4): a cancelled customer can keep
//! every receipt they ever wrote, intact and re-verifiable.
//!
//! Every line goes through `fetch_and_verify` before being emitted, so if a
//! blob is tampered at rest the stream surfaces a `{"error":...}` line
//! instead of the bad attestation. The caller (an OSS audit viewer) can then
//! see the failure in-band.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use futures::stream;
use serde_json::json;

use crate::auth::AuthedAccount;
use crate::error::ApiResult;
use crate::handlers::attestations::fetch_and_verify;
use crate::state::AppState;

/// Streams `application/x-ndjson`.
pub async fn stream(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
) -> ApiResult<impl IntoResponse> {
    let rows = state.db.list_all_attestations(account.id).await?;

    let stream = stream::unfold((state, rows.into_iter()), |(state, mut iter)| async move {
        let row = iter.next()?;
        let line = match fetch_and_verify(&state, &row).await {
            Ok(att) => {
                let mut s = serde_json::to_string(&att)
                    .unwrap_or_else(|_| r#"{"error":"serialize_failed"}"#.to_string());
                s.push('\n');
                s
            }
            Err(e) => {
                let mut s = json!({
                    "error": "signature_invalid",
                    "detail": e.to_string(),
                    "attestation_id": row.id,
                })
                .to_string();
                s.push('\n');
                s
            }
        };
        Some((Ok::<_, std::convert::Infallible>(line), (state, iter)))
    });

    let body = Body::from_stream(stream);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"awp-attestations.jsonl\"",
            ),
        ],
        body,
    ))
}
