//! Integration coverage for the Step 5 anonymous telemetry surface.
//!
//! Two surfaces under test:
//!
//! * `POST /v1/telemetry/usage` — unauthenticated daily aggregate ingest.
//!   No `x-api-key` is permitted on this route (the SDK posts before any
//!   user has signed up), and the payload schema is the one the SDK ships
//!   against — adding fields is forwards-compatible; renames are not.
//! * `GET /v1/admin/telemetry/conversion-ready` — admin-only view of
//!   FileSink installs above the daily threshold called out in
//!   `planning/gtm-phase-2-plan.md` Step 5.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{body_json, Harness};
use serde_json::{json, Value};
use uuid::Uuid;

fn post_telemetry(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/telemetry/usage")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get_admin(uri: &str, key: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("x-admin-key", key)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn telemetry_ingest_accepts_well_formed_event_without_auth() {
    let h = Harness::new().await;
    let req = post_telemetry(json!({
        "install_id": Uuid::new_v4(),
        "sdk_version": "0.1.0",
        "attestations_emitted_today": 42,
        "sink_type": "FileSink",
        "python_version": "3.11.5",
        "os": "darwin",
    }));
    let resp = h.send(req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn telemetry_ingest_rejects_negative_count() {
    let h = Harness::new().await;
    let req = post_telemetry(json!({
        "install_id": Uuid::new_v4(),
        "sdk_version": "0.1.0",
        "attestations_emitted_today": -3,
        "sink_type": "FileSink",
        "python_version": "3.11",
        "os": "linux",
    }));
    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_request");
}

#[tokio::test]
async fn telemetry_admin_view_lists_high_volume_filesink_installs() {
    let h = Harness::new().await;

    // Two installs: one quiet, one conversion-ready.
    let quiet = Uuid::new_v4();
    let big = Uuid::new_v4();
    h.send(post_telemetry(json!({
        "install_id": quiet,
        "sdk_version": "0.1.0",
        "attestations_emitted_today": 50,
        "sink_type": "FileSink",
        "python_version": "3.11",
        "os": "darwin",
    })))
    .await;
    h.send(post_telemetry(json!({
        "install_id": big,
        "sdk_version": "0.1.0",
        "attestations_emitted_today": 12_345,
        "sink_type": "FileSink",
        "python_version": "3.11",
        "os": "linux",
    })))
    .await;
    // A CloudSink user above threshold — should be excluded (already
    // paying; the dashboard is for OSS conversion targets).
    h.send(post_telemetry(json!({
        "install_id": Uuid::new_v4(),
        "sdk_version": "0.1.0",
        "attestations_emitted_today": 99_999,
        "sink_type": "CloudSink",
        "python_version": "3.11",
        "os": "linux",
    })))
    .await;

    let req = get_admin(
        "/v1/admin/telemetry/conversion-ready",
        &h.state.billing.admin_key,
    );
    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["min_attestations"], 10_000);
    let installs = body["installs"].as_array().unwrap();
    assert_eq!(installs.len(), 1, "only the big FileSink install qualifies");
    assert_eq!(installs[0]["install_id"], big.to_string());
    assert_eq!(installs[0]["attestations_emitted_today"], 12_345);
}

#[tokio::test]
async fn telemetry_admin_rejects_missing_admin_key() {
    let h = Harness::new().await;
    let req = Request::builder()
        .method("GET")
        .uri("/v1/admin/telemetry/conversion-ready")
        .body(Body::empty())
        .unwrap();
    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn telemetry_admin_rejects_wrong_admin_key() {
    let h = Harness::new().await;
    let req = get_admin("/v1/admin/telemetry/conversion-ready", "not-the-admin-key");
    let (status, _body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn telemetry_ingest_upserts_idempotently_on_redelivery() {
    let h = Harness::new().await;
    let id = Uuid::new_v4();
    let payload = |n: i64| {
        json!({
            "install_id": id,
            "sdk_version": "0.1.0",
            "attestations_emitted_today": n,
            "sink_type": "FileSink",
            "python_version": "3.11",
            "os": "darwin",
        })
    };

    // First post lands the row.
    h.send(post_telemetry(payload(100))).await;
    // Second post for the same (install_id, day) overwrites, not accumulates,
    // because the SDK sends a daily aggregate not an increment.
    h.send(post_telemetry(payload(50_000))).await;

    let req = get_admin(
        "/v1/admin/telemetry/conversion-ready",
        &h.state.billing.admin_key,
    );
    let (status, body) = body_json(h.send(req).await).await;
    assert_eq!(status, StatusCode::OK);
    let installs = body["installs"].as_array().unwrap();
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0]["attestations_emitted_today"], 50_000);
}
