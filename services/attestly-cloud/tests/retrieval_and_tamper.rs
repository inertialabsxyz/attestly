//! `GET /v1/attestations/{id}` — happy path, account isolation, and the
//! load-bearing tamper-at-rest guarantee.

mod common;

use attestly_cloud::canonical::{blob_sha256, canonical_blob_bytes};
use attestly_core::signing::AgentKeypair;
use attestly_core::Attestation;
use axum::http::StatusCode;
use common::{body_json, get_authed, make_signed_attestation, post_json, Harness};

async fn ingest_one(h: &Harness) -> Attestation {
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-tamper", "CUST-T1");
    let resp = h
        .send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(&att).unwrap(),
        ))
        .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    att
}

#[tokio::test]
async fn fetch_returns_full_canonical_attestation() {
    let h = Harness::new().await;
    let original = ingest_one(&h).await;
    let resp = h
        .send(get_authed(
            &format!("/v1/attestations/{}", original.id),
            &h.api_key,
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let returned: Attestation = serde_json::from_value(body).unwrap();
    assert_eq!(returned, original);
}

#[tokio::test]
async fn fetch_returns_404_for_unknown_id() {
    let h = Harness::new().await;
    let id = uuid::Uuid::new_v4();
    let resp = h
        .send(get_authed(&format!("/v1/attestations/{id}"), &h.api_key))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn fetch_returns_404_for_other_accounts_attestation() {
    // Account A creates an attestation; Account B must not be able to read it
    // by ID. The 404 (not 401, not 403) is deliberate — we don't leak
    // existence.
    let h = Harness::new().await;
    let original = ingest_one(&h).await;

    let (_b_acct, b_key) = h.second_account().await;
    let resp = h
        .send(get_authed(
            &format!("/v1/attestations/{}", original.id),
            &b_key,
        ))
        .await;
    let (status, _) = body_json(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tamper_at_rest_surfaces_as_422() {
    // Simulate a corruption between write and read. The server must refuse
    // to return the tampered bytes; auditors rely on this contract.
    let h = Harness::new().await;
    let original = ingest_one(&h).await;

    // Mutate the stored blob — write a *different* attestation under the
    // *same* SHA-256 key. The content-address check would already catch this
    // (the bytes' SHA won't match the stored hash), but we make the test
    // concrete by writing arbitrary bytes.
    let canonical = canonical_blob_bytes(&original);
    let sha_hex = hex::encode(blob_sha256(&canonical));
    h.state
        .blob
        .tamper_for_test(&sha_hex, b"<<garbage>>")
        .await
        .unwrap();

    let resp = h
        .send(get_authed(
            &format!("/v1/attestations/{}", original.id),
            &h.api_key,
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "tampered at-rest blob must surface 422"
    );
    assert_eq!(body["error"], "signature_invalid");
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("content address") || detail.contains("possible tamper"),
        "detail must hint at tamper: {detail}"
    );
}

#[tokio::test]
async fn tamper_with_valid_resigning_still_fails() {
    // Subtler attack: someone produces a *valid* attestation under a
    // different identity and writes it under the *original*'s blob key.
    // The content-address check rescues us before the signature check ever
    // runs — the new bytes hash differently from the stored hash.
    let h = Harness::new().await;
    let original = ingest_one(&h).await;

    let evil_kp = AgentKeypair::generate();
    let evil = make_signed_attestation(&evil_kp, "agent-evil", "CUST-T1");
    let evil_bytes = canonical_blob_bytes(&evil);

    let sha_hex = hex::encode(blob_sha256(&canonical_blob_bytes(&original)));
    h.state
        .blob
        .tamper_for_test(&sha_hex, &evil_bytes)
        .await
        .unwrap();

    let resp = h
        .send(get_authed(
            &format!("/v1/attestations/{}", original.id),
            &h.api_key,
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "signature_invalid");
}
