//! Share-link routes: create, public JSON read, HTML render, revoke,
//! cross-account isolation, and expiry.

mod common;

use attestly_core::signing::AgentKeypair;
use attestly_core::Attestation;
use axum::http::StatusCode;
use common::{body_json, body_text, get_public, make_signed_attestation, post_json, Harness};
use serde_json::json;

async fn ingest_one(h: &Harness) -> Attestation {
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-share", "CUST-S1");
    h.send(post_json(
        "/v1/attestations",
        &h.api_key,
        serde_json::to_value(&att).unwrap(),
    ))
    .await;
    att
}

#[tokio::test]
async fn create_share_link_with_ids_returns_token() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({"attestation_ids": [att.id.to_string()]}),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["token"].as_str().unwrap().len() >= 40);
    assert!(body["url"].as_str().unwrap().contains("/share/"));
}

#[tokio::test]
async fn share_link_redeems_publicly_with_no_auth() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({"attestation_ids": [att.id.to_string()]}),
        ))
        .await;
    let (_, body) = body_json(resp).await;
    let token = body["token"].as_str().unwrap().to_string();

    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let atts = body["attestations"].as_array().unwrap();
    assert_eq!(atts.len(), 1);
    let returned: Attestation = serde_json::from_value(atts[0].clone()).unwrap();
    assert_eq!(returned, att);
}

#[tokio::test]
async fn share_html_page_renders_atts() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({"attestation_ids": [att.id.to_string()]}),
        ))
        .await;
    let (_, body) = body_json(resp).await;
    let token = body["token"].as_str().unwrap().to_string();

    let resp = h.send(get_public(&format!("/share/{token}"))).await;
    let (status, text) = body_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        text.contains("Attestly shared receipts"),
        "page must render the shared receipts header"
    );
    assert!(
        text.contains(&att.id.to_string()),
        "page must inline the attestation id"
    );
    assert!(
        text.contains("/static/ed25519.js"),
        "page must point at the vendored verifier"
    );
}

#[tokio::test]
async fn share_link_rejects_other_accounts_ids() {
    let h = Harness::new().await;
    let _mine = ingest_one(&h).await;
    let (_other_acct, other_key) = h.second_account().await;

    // Their key tries to share *my* attestation by id — must 404.
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &other_key,
            json!({"attestation_ids": [_mine.id.to_string()]}),
        ))
        .await;
    let (status, _) = body_json(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoked_share_link_returns_404() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({"attestation_ids": [att.id.to_string()]}),
        ))
        .await;
    let (_, body) = body_json(resp).await;
    let token = body["token"].as_str().unwrap().to_string();

    let resp = h
        .send(
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!("/v1/share-links/{token}"))
                .header("x-api-key", &h.api_key)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "link_not_found");
}

#[tokio::test]
async fn share_link_with_filter_resolves_matching_ids() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let a = make_signed_attestation(&kp, "agent-x", "FILTER-1");
    let b = make_signed_attestation(&kp, "agent-y", "FILTER-2");
    for att in [&a, &b] {
        h.send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(att).unwrap(),
        ))
        .await;
    }

    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({"filter": {"agent_id": "agent-x"}}),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = body["token"].as_str().unwrap();
    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (_, body) = body_json(resp).await;
    let atts = body["attestations"].as_array().unwrap();
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0]["agent_id"], "agent-x");
}
