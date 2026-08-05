//! End-to-end tamper evidence across the **share-link** surface
//! (`docs/PRODUCTION_CHECKLIST.md` §4, last bullet).
//!
//! `retrieval_and_tamper.rs` already covers the owner-facing retrieval path
//! (`GET /v1/attestations/{id}` → 422). This file covers the path that
//! actually matters to the guarantee, because it is the one an outside auditor
//! uses: a share link. The full chain is exercised in one test — ingest →
//! create share link → mutate the at-rest blob → redeem — and both redemption
//! surfaces are asserted:
//!
//! - `GET /v1/share-links/{token}` (JSON) → **422** with `signature_invalid`
//! - `GET /share/{token}` (HTML) → the **red-banner** tamper page
//!
//! The HTML surface is the load-bearing one. It is served on the *success*
//! path of an axum handler (`share_links::render_page` catches the
//! `SignatureInvalid` error and returns `200 OK` with the banner, so an
//! auditor sees the tamper signal rather than an opaque HTTP error), which
//! means a regression here would **not** show up as a failing status code.
//! Only asserting on the page content catches it.
//!
//! Step 4 confirms the same guarantee against the live host; this is the
//! in-repo regression guard.

mod common;

use attestly_cloud::canonical::{blob_sha256, canonical_blob_bytes};
use attestly_cloud::web::TAMPERED_PAGE_MARKER;
use attestly_core::signing::AgentKeypair;
use attestly_core::Attestation;
use axum::http::StatusCode;
use common::{body_json, body_text, get_public, make_signed_attestation, post_json, Harness};
use serde_json::json;

/// Ingest one signed attestation and return it.
async fn ingest_one(h: &Harness) -> Attestation {
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-tamper-e2e", "CUST-TE1");
    let resp = h
        .send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(&att).unwrap(),
        ))
        .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "ingest must succeed");
    att
}

/// Mint a share link over `att` and return its token.
async fn share(h: &Harness, att: &Attestation) -> String {
    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({ "attestation_ids": [att.id.to_string()] }),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED, "share-link creation succeeds");
    body["token"].as_str().unwrap().to_string()
}

/// Corrupt the at-rest blob backing `att`, as a disk fault or a compromised
/// operator would.
async fn tamper(h: &Harness, att: &Attestation) {
    let sha_hex = hex::encode(blob_sha256(&canonical_blob_bytes(att)));
    h.state
        .blob
        .tamper_for_test(&sha_hex, b"<<tampered at rest>>")
        .await
        .unwrap();
}

#[tokio::test]
async fn shared_attestation_verifies_before_tampering() {
    // The control. Without this, the tamper assertions below could pass
    // against a link that was broken from the start.
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let token = share(&h, &att).await;

    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["attestations"].as_array().unwrap().len(), 1);

    let resp = h.send(get_public(&format!("/share/{token}"))).await;
    let (status, html) = body_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !html.contains(TAMPERED_PAGE_MARKER),
        "an untampered share link must not render the tamper banner"
    );
}

#[tokio::test]
async fn tampered_blob_surfaces_422_signature_invalid_on_the_share_json() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let token = share(&h, &att).await;
    tamper(&h, &att).await;

    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a tampered blob must surface 422 on the share JSON, never the bytes"
    );
    assert_eq!(
        body["error"], "signature_invalid",
        "422 must carry the `signature_invalid` slug from API.md"
    );
}

#[tokio::test]
async fn tampered_blob_renders_the_red_banner_share_page() {
    let h = Harness::new().await;
    let att = ingest_one(&h).await;
    let token = share(&h, &att).await;
    tamper(&h, &att).await;

    let resp = h.send(get_public(&format!("/share/{token}"))).await;
    let (status, html) = body_text(resp).await;

    // Deliberately 200, not 422: the page is the error surface. An auditor
    // gets a page that says "verification failed" rather than a bare HTTP
    // error their browser renders as nothing useful.
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(TAMPERED_PAGE_MARKER),
        "the tampered share page must carry the stable tamper marker; got:\n{html}"
    );
    assert!(
        html.contains("Verification failed"),
        "the tampered share page must say so in human-readable terms"
    );

    // The whole point of refusing: the receipt payload must not reach the
    // page. If re-verification failed, the bytes are not trustworthy and must
    // not be rendered alongside a warning the reader might miss.
    assert!(
        !html.contains(&att.agent_id),
        "the tampered page must not render the underlying receipt"
    );
}

#[tokio::test]
async fn tampering_one_attestation_refuses_the_whole_share_link() {
    // A link covering several receipts must fail closed. Rendering the intact
    // ones beside a banner would invite an auditor to trust a page that is
    // partly unverified.
    let h = Harness::new().await;
    let good = ingest_one(&h).await;

    let kp = AgentKeypair::generate();
    let bad = make_signed_attestation(&kp, "agent-tamper-e2e", "CUST-TE2");
    let resp = h
        .send(post_json(
            "/v1/attestations",
            &h.api_key,
            serde_json::to_value(&bad).unwrap(),
        ))
        .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = h
        .send(post_json(
            "/v1/share-links",
            &h.api_key,
            json!({ "attestation_ids": [good.id.to_string(), bad.id.to_string()] }),
        ))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = body["token"].as_str().unwrap().to_string();

    // Corrupt only the second one.
    tamper(&h, &bad).await;

    let resp = h
        .send(get_public(&format!("/v1/share-links/{token}")))
        .await;
    let (status, body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "one tampered member must fail the whole link"
    );
    assert_eq!(body["error"], "signature_invalid");

    let resp = h.send(get_public(&format!("/share/{token}"))).await;
    let (_, html) = body_text(resp).await;
    assert!(
        html.contains(TAMPERED_PAGE_MARKER),
        "the page must show the tamper banner, not the surviving receipts"
    );
}
