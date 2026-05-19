//! Integration coverage for the Step 5 self-serve signup + quickstart page.
//!
//! These tests exercise the surface the quickstart depends on:
//!
//! * `POST /v1/account/signup` round-trips an email + password into a Team
//!   account with a freshly-minted API key
//! * the issued key works against the rest of the `/v1/` surface (the
//!   load-bearing claim — the quickstart is broken if the key can't ingest)
//! * email collisions are rejected with a stable error shape
//! * `GET /quickstart` serves an HTML shell containing the load-bearing
//!   copy and the polling JS that drives the "first attestation" moment

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{body_json, make_signed_attestation, post_json, Harness};
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn signup_creates_team_account_and_returns_api_key() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": "new@user.com", "password": "longenough"}).to_string(),
        ))
        .unwrap();

    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["email"], "new@user.com");
    assert_eq!(body["plan"], "team");
    let api_key = body["api_key"]
        .as_str()
        .expect("api_key returned")
        .to_string();
    assert_eq!(api_key.len(), 36, "api key should be a uuid");
    assert!(body["dashboard_url"]
        .as_str()
        .expect("dashboard_url")
        .ends_with("/dashboard?welcome=1"));

    // The minted key must work against the rest of the v1 surface — the
    // load-bearing claim of the quickstart.
    let me = Request::builder()
        .method("GET")
        .uri("/v1/account")
        .header("x-api-key", &api_key)
        .body(Body::empty())
        .unwrap();
    let (status, body) = body_json(h.send(me).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "new@user.com");
}

#[tokio::test]
async fn signup_rejects_short_password() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": "u@d.com", "password": "short"}).to_string(),
        ))
        .unwrap();
    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_request");
}

#[tokio::test]
async fn signup_rejects_invalid_email() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": "not-an-email", "password": "longenough"}).to_string(),
        ))
        .unwrap();
    let (status, _body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn signup_rejects_duplicate_email() {
    let h = Harness::new().await;
    let payload = json!({"email": "dup@user.com", "password": "longenough"});
    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let (status1, _) = body_json(h.send(req1).await).await;
    assert_eq!(status1, StatusCode::CREATED);

    let req2 = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let (status2, body) = body_json(h.send(req2).await).await;
    assert_eq!(status2, StatusCode::BAD_REQUEST);
    assert!(body["detail"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn signup_key_ingests_and_dashboard_polling_sees_it() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/account/signup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": "ingest@me.com", "password": "longenough"}).to_string(),
        ))
        .unwrap();
    let (_, body) = body_json(h.send(req).await).await;
    let api_key = body["api_key"].as_str().unwrap().to_string();

    // The quickstart page polls `/v1/account/usage`. Before any
    // attestation it must return zero points.
    let usage_before = Request::builder()
        .method("GET")
        .uri("/v1/account/usage")
        .header("x-api-key", &api_key)
        .body(Body::empty())
        .unwrap();
    let (status, body) = body_json(h.send(usage_before).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["points"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "usage should be empty before the first attestation"
    );

    // Ingest one attestation under the quickstart key.
    let kp = awp_core::signing::AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "quickstart-agent", "demo-customer");
    let ingest = post_json(
        "/v1/attestations",
        &api_key,
        serde_json::to_value(&att).unwrap(),
    );
    let (status, _) = body_json(h.send(ingest).await).await;
    assert_eq!(status, StatusCode::CREATED);

    // The polling endpoint must now reflect the count.
    let usage_after = Request::builder()
        .method("GET")
        .uri("/v1/account/usage")
        .header("x-api-key", &api_key)
        .body(Body::empty())
        .unwrap();
    let (status, body) = body_json(h.send(usage_after).await).await;
    assert_eq!(status, StatusCode::OK);
    let total: i64 = body["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["count"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(total, 1, "the first-attestation moment must be observable");
}

#[tokio::test]
async fn quickstart_page_renders_with_required_copy() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("GET")
        .uri("/quickstart")
        .body(Body::empty())
        .unwrap();
    let resp = h.send(req).await;
    let (parts, body) = resp.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);

    // Load-bearing copy and DOM hooks for the four-step flow.
    assert!(text.contains("pip install awp-langgraph"));
    assert!(text.contains("/v1/account/signup"));
    assert!(text.contains("from awp.langgraph import attest"));
    assert!(text.contains("startPolling"));
    assert!(text.contains("/v1/account/usage"));
}
