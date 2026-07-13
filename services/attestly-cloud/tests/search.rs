//! `GET /v1/attestations` — paginated search.

mod common;

use attestly_core::signing::AgentKeypair;
use axum::http::StatusCode;
use common::{body_json, get_authed, make_signed_attestation, post_json, Harness};

async fn seed_n(h: &Harness, n: usize, agent_id: &str, customer_id: &str) {
    let kp = AgentKeypair::generate();
    for i in 0..n {
        let cid = format!("{customer_id}-{i:04}");
        let att = make_signed_attestation(&kp, agent_id, &cid);
        let resp = h
            .send(post_json(
                "/v1/attestations",
                &h.api_key,
                serde_json::to_value(&att).unwrap(),
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}

#[tokio::test]
async fn search_returns_paginated_results() {
    let h = Harness::new().await;
    seed_n(&h, 125, "agent-search", "C").await;

    let resp = h
        .send(get_authed("/v1/attestations?limit=50", &h.api_key))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["attestations"].as_array().unwrap().len(), 50);
    let cursor1 = body["next_cursor"]
        .as_str()
        .expect("first page must have cursor")
        .to_string();

    let resp = h
        .send(get_authed(
            &format!("/v1/attestations?limit=50&cursor={cursor1}"),
            &h.api_key,
        ))
        .await;
    let (_, body) = body_json(resp).await;
    assert_eq!(body["attestations"].as_array().unwrap().len(), 50);
    let cursor2 = body["next_cursor"].as_str().unwrap().to_string();

    let resp = h
        .send(get_authed(
            &format!("/v1/attestations?limit=50&cursor={cursor2}"),
            &h.api_key,
        ))
        .await;
    let (_, body) = body_json(resp).await;
    assert_eq!(body["attestations"].as_array().unwrap().len(), 25);
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn search_filters_by_agent_id() {
    let h = Harness::new().await;
    seed_n(&h, 5, "agent-a", "X").await;
    seed_n(&h, 3, "agent-b", "Y").await;

    let resp = h
        .send(get_authed(
            "/v1/attestations?agent_id=agent-a&limit=100",
            &h.api_key,
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["attestations"].as_array().unwrap().len(), 5);
    for row in body["attestations"].as_array().unwrap() {
        assert_eq!(row["agent_id"], "agent-a");
    }
}

#[tokio::test]
async fn search_filters_by_customer_id() {
    let h = Harness::new().await;
    let kp = AgentKeypair::generate();
    let target_cid = "CUST-TARGET-001";
    let target = make_signed_attestation(&kp, "agent-c", target_cid);
    let other = make_signed_attestation(&kp, "agent-c", "CUST-OTHER-002");
    for att in [&target, &other] {
        h.send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(att).unwrap(),
        ))
        .await;
    }

    let resp = h
        .send(get_authed(
            &format!("/v1/attestations?customer_id={target_cid}"),
            &h.api_key,
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body["attestations"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], target.id.to_string());
}

#[tokio::test]
async fn search_rejects_excessive_limit() {
    let h = Harness::new().await;
    let resp = h
        .send(get_authed("/v1/attestations?limit=10000", &h.api_key))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_request");
}

#[tokio::test]
async fn search_isolates_between_accounts() {
    let h = Harness::new().await;
    seed_n(&h, 4, "agent-mine", "Z").await;

    let (_, other_key) = h.second_account().await;
    let resp = h.send(get_authed("/v1/attestations", &other_key)).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["attestations"].as_array().unwrap().len(),
        0,
        "other account must not see my attestations"
    );
}
