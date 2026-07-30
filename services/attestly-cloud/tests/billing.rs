//! Integration tests for Step 4 — pricing, billing, retention, dashboard.
//!
//! The Stripe surface is exercised against [`MockStripeClient`], which
//! records every call and returns deterministic ids. Webhook signing is
//! exercised end-to-end against the real HMAC-SHA256 path via
//! [`stripe::webhook::sign_for_test`]; only the network hop to Stripe is
//! mocked out.

mod common;

use std::sync::Arc;

use attestly_cloud::store::{AttestationIndex, Plan};
use attestly_cloud::stripe::webhook;
use attestly_core::signing::AgentKeypair;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use common::{body_json, body_text, get_authed, make_signed_attestation, post_json, Harness};
use serde_json::{json, Value};
use uuid::Uuid;

fn signed_webhook(secret: &str, event: &Value) -> (Vec<u8>, String) {
    let payload = serde_json::to_vec(event).unwrap();
    let ts = Utc::now().timestamp();
    let header = webhook::sign_for_test(&payload, secret, ts);
    (payload, header)
}

fn webhook_request(payload: Vec<u8>, header: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/v1/billing/webhook")
        .header("content-type", "application/json");
    if let Some(h) = header {
        b = b.header("stripe-signature", h);
    }
    b.body(Body::from(payload)).unwrap()
}

#[tokio::test]
async fn webhook_rejects_invalid_signature() {
    let h = Harness::new().await;
    let event = json!({
        "id": "evt_1",
        "type": "checkout.session.completed",
        "data": { "object": {} }
    });
    let payload = serde_json::to_vec(&event).unwrap();
    // Current timestamp (passes the replay window) but a v1 signature
    // computed under the wrong secret. Verification must refuse.
    let ts = Utc::now().timestamp();
    let bad_header = webhook::sign_for_test(&payload, "whsec_wrong", ts);
    let resp = h.send(webhook_request(payload, Some(&bad_header))).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn webhook_accepts_signed_checkout_and_provisions_account() {
    let h = Harness::new().await;
    let event = json!({
        "id": "evt_checkout_1",
        "type": "checkout.session.completed",
        "data": {
            "object": {
                "id": "cs_test_1",
                "customer": "cus_test_1",
                "subscription": "sub_test_1",
                "customer_details": { "email": "newteam@example.com" }
            }
        }
    });
    let (payload, header) = signed_webhook("whsec_test", &event);
    let resp = h.send(webhook_request(payload, Some(&header))).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Account row should exist with the Stripe linkage and a default API key
    // — both are observable through the Db trait the handler used.
    let accts = h.state.db.list_accounts().await.unwrap();
    let provisioned = accts
        .iter()
        .find(|a| a.email == "newteam@example.com")
        .expect("checkout webhook must have created an account");
    assert_eq!(provisioned.plan, Plan::Team);
    assert_eq!(
        provisioned.stripe_customer_id.as_deref(),
        Some("cus_test_1")
    );
    assert_eq!(
        provisioned.stripe_subscription_id.as_deref(),
        Some("sub_test_1")
    );
    let keys = h
        .state
        .db
        .list_active_api_keys(provisioned.id)
        .await
        .unwrap();
    assert_eq!(keys.len(), 1, "exactly one default key issued");
}

#[tokio::test]
async fn checkout_webhook_is_idempotent_on_redelivery() {
    let h = Harness::new().await;
    let event = json!({
        "id": "evt_checkout_2",
        "type": "checkout.session.completed",
        "data": {
            "object": {
                "id": "cs_test_2",
                "customer": "cus_test_2",
                "subscription": "sub_test_2",
                "customer_details": { "email": "dup@example.com" }
            }
        }
    });
    let (payload, header) = signed_webhook("whsec_test", &event);
    h.send(webhook_request(payload.clone(), Some(&header)))
        .await;
    h.send(webhook_request(payload, Some(&header))).await;

    let accts = h.state.db.list_accounts().await.unwrap();
    let dup_count = accts
        .iter()
        .filter(|a| a.email == "dup@example.com")
        .count();
    assert_eq!(dup_count, 1, "re-delivery must not double-create accounts");
    let acct = accts.iter().find(|a| a.email == "dup@example.com").unwrap();
    let keys = h.state.db.list_active_api_keys(acct.id).await.unwrap();
    assert_eq!(keys.len(), 1, "re-delivery must not issue a second key");
}

#[tokio::test]
async fn checkout_endpoint_redirects_to_stripe() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("GET")
        .uri("/billing/checkout?plan=team")
        .body(Body::empty())
        .unwrap();
    let resp = h.send(req).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        loc.starts_with("https://stripe.test/checkout/"),
        "expected redirect to mock checkout URL, got {loc}"
    );
    let recorded = h.stripe.checkouts();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].price_id, "price_test");
}

#[tokio::test]
async fn portal_requires_stripe_customer_on_account() {
    let h = Harness::new().await;
    // The default Harness account has no Stripe customer attached.
    let resp = h
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/portal")
                .header("x-api-key", &h.api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn portal_returns_stripe_url_for_linked_account() {
    let h = Harness::new().await;
    h.state
        .db
        .set_account_stripe_ids(h.account.id, "cus_linked", Some("sub_linked"))
        .await
        .unwrap();
    let resp = h
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/portal")
                .header("x-api-key", &h.api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["url"]
        .as_str()
        .unwrap()
        .starts_with("https://stripe.test/portal/"));
    let portals = h.stripe.portals();
    assert_eq!(portals.len(), 1);
    assert_eq!(portals[0].customer_id, "cus_linked");
}

#[tokio::test]
async fn admin_bill_period_requires_admin_key() {
    let h = Harness::new().await;
    let resp = h
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/admin/bill-period")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_bill_period_posts_overage_for_synthetic_account() {
    let h = Harness::new().await;
    // Seed 1.05M attestations of usage for the harness account against a
    // specific calendar month.
    let from = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let day = from.and_hms_opt(12, 0, 0).unwrap().and_utc();
    // 1,050,000 attestations on one day — overage = 50,000 → 50 units of 1k.
    for _ in 0..1_050_000 {
        h.state.db.record_usage(h.account.id, day).await.unwrap();
    }

    let to = chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let body = json!({"from": from, "to": to});
    let resp = h
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/admin/bill-period")
                .header("content-type", "application/json")
                .header("x-admin-key", "admin-test")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await;
    let (status, payload) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let lines = payload["results"].as_array().unwrap();
    let line = lines
        .iter()
        .find(|l| l["account_id"].as_str().unwrap() == h.account.id.to_string())
        .expect("our account must appear in the results");
    assert_eq!(line["overage_units"].as_i64().unwrap(), 50);
    assert!(line["usage_record_id"].as_str().is_some());

    let usage_records = h.stripe.usage_records();
    assert_eq!(usage_records.len(), 1);
    assert_eq!(usage_records[0].quantity, 50);

    // Replay the trigger: must be a no-op, no second Stripe call.
    let body = json!({"from": from, "to": to});
    h.send(
        Request::builder()
            .method("POST")
            .uri("/v1/admin/bill-period")
            .header("content-type", "application/json")
            .header("x-admin-key", "admin-test")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(
        h.stripe.usage_records().len(),
        1,
        "second run must not post another Stripe usage record"
    );
}

#[tokio::test]
async fn admin_bill_period_skips_account_within_floor() {
    let h = Harness::new().await;
    let from = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let day = from.and_hms_opt(12, 0, 0).unwrap().and_utc();
    // Only 100 attestations — well below the 1M floor.
    for _ in 0..100 {
        h.state.db.record_usage(h.account.id, day).await.unwrap();
    }
    let to = chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let body = json!({"from": from, "to": to});
    let resp = h
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/admin/bill-period")
                .header("content-type", "application/json")
                .header("x-admin-key", "admin-test")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await;
    let (status, _) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        h.stripe.usage_records().is_empty(),
        "no Stripe call for accounts within the included floor"
    );
}

#[tokio::test]
async fn dashboard_html_renders() {
    let h = Harness::new().await;
    let resp = h
        .send(
            Request::builder()
                .method("GET")
                .uri("/dashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let (status, text) = body_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("Attestly Cloud — Dashboard"));
    // Surfaces all three management areas the spec calls out.
    assert!(text.contains("Plan"));
    assert!(text.contains("Usage"));
    assert!(text.contains("API keys"));
    assert!(text.contains("Manage subscription"));
    assert!(text.contains("Export everything"));
}

#[tokio::test]
async fn account_summary_returns_plan_and_usage() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-dash", "dash-1");
    h.send(post_json(
        "/v1/attestations",
        &h.api_key,
        serde_json::to_value(&att).unwrap(),
    ))
    .await;
    let resp = h.send(get_authed("/v1/account", &h.api_key)).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["plan"], "team");
    assert_eq!(body["retention_days"], 365);
    assert_eq!(body["usage_this_period"], 1);
    assert_eq!(body["included_attestations"], 1_000_000);
}

#[tokio::test]
async fn create_and_revoke_api_key_round_trip() {
    let h = Harness::new().await;
    let resp = h
        .send(post_json(
            "/v1/account/api-keys",
            &h.api_key,
            json!({"project_name": "new-project"}),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED);
    let key_id = body["id"].as_str().unwrap().to_string();
    let new_cleartext = body["key"].as_str().unwrap().to_string();
    assert!(!new_cleartext.is_empty());

    // The new key must authenticate.
    let resp = h.send(get_authed("/v1/account", &new_cleartext)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Revoke and confirm the key no longer authenticates.
    let resp = h
        .send(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/account/api-keys/{key_id}"))
                .header("x-api-key", &h.api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = h.send(get_authed("/v1/account", &new_cleartext)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn export_then_audit_viewer_can_re_render() {
    // The export endpoint is already integration-tested for shape in
    // tests/export_and_health.rs. This test confirms the round-trip
    // requirement from the Step 4 spec: every line is a self-contained
    // Attestation that the static audit viewer's verification path
    // accepts. The "audit viewer accepts" condition is structurally:
    // every line parses as `Attestation` and verifies against its
    // embedded pubkey — exactly what `fetch_and_verify` proves on the
    // server side, but we re-prove it client-side from the exported
    // bytes alone.
    use attestly_core::Attestation;

    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let originals: Vec<Attestation> = (0..5)
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
    let (status, text) = body_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), originals.len());
    for line in &lines {
        let att: Attestation = serde_json::from_str(line).expect("each line parses");
        // Re-verify against the embedded pubkey using attestly-core directly —
        // this is exactly what the audit viewer's JS does, just in Rust.
        assert!(
            attestly_core::signing::verify_attestation_signature(&att),
            "exported attestation must verify against its embedded pubkey"
        );
    }
}

#[tokio::test]
async fn retention_sweep_deletes_rows_older_than_retention_days() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let now = Utc::now();

    // 100 fresh attestations
    let fresh_count = 100usize;
    for i in 0..fresh_count {
        let att = make_signed_attestation(&kp, "agent-r", &format!("F-{i}"));
        let canon = attestly_cloud::canonical::canonical_blob_bytes(&att);
        let sha = hex::encode(attestly_cloud::canonical::blob_sha256(&canon));
        h.state.blob.put(&sha, &canon).await.unwrap();
        h.state
            .db
            .insert_attestation(AttestationIndex {
                id: att.id,
                account_id: h.account.id,
                agent_id: att.agent_id.clone(),
                agent_pubkey_hex: hex::encode(att.agent_pubkey),
                customer_id: Some(format!("F-{i}")),
                received_at: now,
                blob_sha256_hex: sha,
            })
            .await
            .unwrap();
    }

    // 100 stale attestations dated 400 days ago — older than the 365-day
    // Team retention horizon.
    let stale_count = 100usize;
    let stale_at = now - Duration::days(400);
    for i in 0..stale_count {
        let att = make_signed_attestation(&kp, "agent-r", &format!("S-{i}"));
        let canon = attestly_cloud::canonical::canonical_blob_bytes(&att);
        let sha = hex::encode(attestly_cloud::canonical::blob_sha256(&canon));
        h.state.blob.put(&sha, &canon).await.unwrap();
        h.state
            .db
            .insert_attestation(AttestationIndex {
                id: Uuid::new_v4(),
                account_id: h.account.id,
                agent_id: att.agent_id.clone(),
                agent_pubkey_hex: hex::encode(att.agent_pubkey),
                customer_id: Some(format!("S-{i}")),
                received_at: stale_at,
                blob_sha256_hex: sha,
            })
            .await
            .unwrap();
    }

    let (visited, deleted) = attestly_cloud::sweeper::sweep_once(Arc::clone(&h.state.db))
        .await
        .unwrap();
    assert!(visited >= 1);
    assert_eq!(
        deleted as usize, stale_count,
        "sweep must delete only the stale rows"
    );
    let remaining = h
        .state
        .db
        .list_all_attestations(h.account.id)
        .await
        .unwrap();
    assert_eq!(remaining.len(), fresh_count);
}

#[tokio::test]
async fn retention_sweep_respects_enterprise_horizon() {
    let h = Harness::new().await;
    // Convert the harness account to Enterprise — sweep must keep its old rows.
    h.state
        .db
        .set_account_plan(h.account.id, Plan::Enterprise)
        .await
        .unwrap();
    let kp = AgentKeypair::generate();
    let stale_at = Utc::now() - Duration::days(400);
    let att = make_signed_attestation(&kp, "agent-e", "E-1");
    let canon = attestly_cloud::canonical::canonical_blob_bytes(&att);
    let sha = hex::encode(attestly_cloud::canonical::blob_sha256(&canon));
    h.state.blob.put(&sha, &canon).await.unwrap();
    h.state
        .db
        .insert_attestation(AttestationIndex {
            id: Uuid::new_v4(),
            account_id: h.account.id,
            agent_id: att.agent_id.clone(),
            agent_pubkey_hex: hex::encode(att.agent_pubkey),
            customer_id: Some("E-1".into()),
            received_at: stale_at,
            blob_sha256_hex: sha,
        })
        .await
        .unwrap();
    let (_, deleted) = attestly_cloud::sweeper::sweep_once(Arc::clone(&h.state.db))
        .await
        .unwrap();
    assert_eq!(
        deleted, 0,
        "Enterprise's 7-year horizon must not touch 400-day-old rows"
    );
}
