//! `GET /v1/export` (the "free to leave" guarantee) and `GET /healthz`.

mod common;

use attestly_core::{signing::AgentKeypair, Attestation};
use axum::http::StatusCode;
use common::{
    body_json, body_text, get_authed, get_public, make_signed_attestation, post_json, Harness,
};
use tower::util::ServiceExt;

/// `/healthz` probes Postgres *and* blob storage (see `health.rs`). On a
/// healthy harness both are reachable, so the contract is 200 `status: "ok"`
/// with a version.
#[tokio::test]
async fn healthz_reports_ok() {
    let h = Harness::new().await;
    let resp = h.send(get_public("/healthz")).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert!(
        body["version"].as_str().is_some_and(|v| !v.is_empty()),
        "healthz must report a version"
    );
}

/// The blob half of the contract: when blob storage is unreachable, `/healthz`
/// must report degraded in the same 503 shape used for a DB failure — not 200.
/// Uses an `FsBlobStore` rooted at a regular file, standing in for the Fly
/// volume failing to mount at `BLOB_ROOT`.
#[tokio::test]
async fn healthz_reports_degraded_when_blob_storage_is_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("blob-root-is-a-file");
    std::fs::write(&not_a_dir, b"x").unwrap();

    let h = Harness::new().await;
    let state = attestly_cloud::AppState::new(
        h.state.db.clone(),
        std::sync::Arc::new(attestly_cloud::blob::filesystem::FsBlobStore::new(
            &not_a_dir,
        )),
        h.state.stripe.clone(),
        h.state.billing.clone(),
    );
    let resp = attestly_cloud::router(state)
        .oneshot(get_public("/healthz"))
        .await
        .unwrap();
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "degraded");
    assert!(
        body["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "degraded response must carry a detail"
    );
}

#[tokio::test]
async fn export_streams_all_account_attestations_as_jsonl() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let originals: Vec<Attestation> = (0..3)
        .map(|i| make_signed_attestation(&kp, "agent-export", &format!("E-{i}")))
        .collect();
    for att in &originals {
        h.send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(att).unwrap(),
        ))
        .await;
    }

    let resp = h.send(get_authed("/v1/export", &h.api_key)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(ct, "application/x-ndjson");
    // The browser dashboard relies on this header to treat the stream as a
    // file download rather than navigating to it.
    let cd = resp
        .headers()
        .get(axum::http::header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cd, "attachment; filename=\"attestly-attestations.jsonl\"");

    let (_, text) = body_text(resp).await;
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3);
    // Every line must round-trip back to an Attestation and match one we sent.
    let returned: Vec<Attestation> = lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("each line must be a full Attestation"))
        .collect();
    for original in &originals {
        assert!(
            returned.iter().any(|r| r == original),
            "exported set must contain every ingested attestation"
        );
    }
}

#[tokio::test]
async fn export_requires_api_key() {
    let h = Harness::new().await;
    let resp = h.send(get_public("/v1/export")).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn billing_webhook_rejects_missing_signature() {
    let h = Harness::new().await;
    // Step 4: webhook is wired. With no `Stripe-Signature` header the
    // request must be refused, never reaching the dispatcher.
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/billing/webhook")
        .header("content-type", "application/json")
        .body(axum::body::Body::from("{}"))
        .unwrap();
    let resp = h.send(req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
