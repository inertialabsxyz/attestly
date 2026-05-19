//! `POST /v1/attestations` — ingest path covering happy path, invalid
//! signature, missing auth, and idempotency.

mod common;

use awp_core::signing::AgentKeypair;
use axum::http::StatusCode;
use common::{body_json, make_signed_attestation, post_json, Harness};

#[tokio::test]
async fn ingest_genuine_attestation_returns_201() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-1", "CUST-001");
    let resp = h
        .send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(&att).unwrap(),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    assert_eq!(body["id"], serde_json::Value::String(att.id.to_string()));
    assert!(body["blob_sha256"].as_str().unwrap().len() == 64);
}

#[tokio::test]
async fn ingest_tampered_attestation_returns_422() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let mut att = make_signed_attestation(&kp, "agent-1", "CUST-001");
    // Tamper *after* signing: the embedded signature no longer covers `output`.
    att.output = "evil".to_string();
    let resp = h
        .send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(&att).unwrap(),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "signature_invalid");
}

#[tokio::test]
async fn ingest_missing_api_key_returns_401() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-1", "CUST-001");
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/attestations")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&att).unwrap()))
        .unwrap();
    let resp = h.send(req).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn ingest_invalid_api_key_returns_401() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-1", "CUST-001");
    let resp = h
        .send(post_json(
            "/v1/attestations",
            "not-a-real-key",
            serde_json::to_value(&att).unwrap(),
        ))
        .await;
    let (status, _) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_is_idempotent_on_duplicate_id() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-1", "CUST-001");
    let body = serde_json::to_value(&att).unwrap();

    let first = h
        .send(post_json("/v1/attestations", &h.api_key, body.clone()))
        .await;
    let (status1, _) = body_json(first).await;
    assert_eq!(status1, StatusCode::CREATED);

    let second = h
        .send(post_json("/v1/attestations", &h.api_key, body))
        .await;
    let (status2, body2) = body_json(second).await;
    assert_eq!(status2, StatusCode::OK, "duplicate ingest must be 200");
    assert_eq!(body2["id"], serde_json::Value::String(att.id.to_string()));

    // Usage counter only ticks once.
    let total = h.state.db.account_usage_total(h.account.id).await.unwrap();
    assert_eq!(total, 1);
}
