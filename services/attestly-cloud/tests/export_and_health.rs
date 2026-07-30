//! `GET /v1/export` (the "free to leave" guarantee) and `GET /healthz`.

mod common;

use attestly_core::{signing::AgentKeypair, Attestation};
use axum::http::StatusCode;
use common::{
    body_json, body_text, get_authed, get_public, make_signed_attestation, post_json, Harness,
};

#[tokio::test]
async fn healthz_reports_ok() {
    let h = Harness::new().await;
    let resp = h.send(get_public("/healthz")).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
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
