//! Rate limiting on the two write paths named by
//! `docs/PRODUCTION_CHECKLIST.md` §4: attestation ingest and share-link
//! creation.
//!
//! ## Determinism
//!
//! No wall-clock sleeps. The limiter's bucket starts full and refills at
//! `60s / limit` per request, so with a limit low enough that the whole burst
//! fits inside a test's runtime, the (limit + 1)-th request is refused
//! deterministically — the refill interval is orders of magnitude longer than
//! the in-memory request it would need to outrun.
//!
//! The limit is injected through [`Harness::with_rate_limit`] rather than an
//! env var, so these tests can't perturb the suites running beside them.
//!
//! Note that the buckets live in the **router**, so a test that wants to
//! observe throttling must reuse one router across requests
//! ([`Harness::app`]) rather than calling `Harness::send`, which builds a
//! fresh one per call.

mod common;

use attestly_core::signing::AgentKeypair;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{body_json, make_signed_attestation, post_json, Harness};
use tower::util::ServiceExt;

/// Drive one request through a persistent router so the limiter accumulates.
async fn send(app: &axum::Router, req: Request<Body>) -> axum::http::Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

fn ingest_body(h: &Harness, customer: &str) -> Request<Body> {
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-rl", customer);
    post_json(
        "/v1/attestations",
        &h.api_key,
        serde_json::to_value(&att).unwrap(),
    )
}

#[tokio::test]
async fn ingest_over_the_limit_returns_429_with_the_rate_limited_slug() {
    const LIMIT: u32 = 3;
    let h = Harness::with_rate_limit(LIMIT).await;
    let app = h.app();

    // The full burst must succeed — the limiter must not throttle a client
    // that stays inside its allowance.
    for i in 0..LIMIT {
        let resp = send(&app, ingest_body(&h, &format!("CUST-{i}"))).await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "request {i} is within the limit of {LIMIT} and must succeed"
        );
    }

    // The next one is over budget.
    let resp = send(&app, ingest_body(&h, "CUST-over")).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "request {} exceeds the limit of {LIMIT}",
        LIMIT + 1
    );
    assert_eq!(
        body["error"], "rate_limited",
        "429 must use the `rate_limited` slug from API.md"
    );
    assert!(
        body["detail"].is_string(),
        "429 must carry the uniform {{error, detail}} envelope"
    );
}

#[tokio::test]
async fn share_link_creation_is_rate_limited_too() {
    // §4 names share-link creation alongside ingest: it mints a *public* read
    // grant over private data, so an unbounded loop is the more dangerous of
    // the two.
    const LIMIT: u32 = 2;
    let h = Harness::with_rate_limit(LIMIT).await;
    let app = h.app();

    let body = serde_json::json!({ "filter": { "agent_id": "agent-rl" } });
    let mut statuses = Vec::new();
    for _ in 0..=LIMIT {
        let resp = send(&app, post_json("/v1/share-links", &h.api_key, body.clone())).await;
        statuses.push(resp.status());
    }

    let (last, earlier) = statuses.split_last().unwrap();
    for (i, s) in earlier.iter().enumerate() {
        assert_ne!(
            *s,
            StatusCode::TOO_MANY_REQUESTS,
            "request {i} is within the limit of {LIMIT} and must not be throttled"
        );
    }
    assert_eq!(
        *last,
        StatusCode::TOO_MANY_REQUESTS,
        "request {} exceeds the limit of {LIMIT}",
        LIMIT + 1
    );
}

#[tokio::test]
async fn one_tenant_cannot_exhaust_anothers_budget() {
    // The isolation property §4 is really after. Account A burns its whole
    // allowance; account B must still be served.
    const LIMIT: u32 = 2;
    let h = Harness::with_rate_limit(LIMIT).await;
    let app = h.app();

    for i in 0..=LIMIT {
        send(&app, ingest_body(&h, &format!("CUST-{i}"))).await;
    }
    // A is now throttled.
    let resp = send(&app, ingest_body(&h, "CUST-a-again")).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // B has an untouched bucket.
    let (_b_acct, b_key) = h.second_account().await;
    let kp = AgentKeypair::generate();
    let att = make_signed_attestation(&kp, "agent-rl-b", "CUST-B1");
    let req = post_json(
        "/v1/attestations",
        &b_key,
        serde_json::to_value(&att).unwrap(),
    );
    let resp = send(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "a second tenant must not be throttled by the first's traffic"
    );
}

#[tokio::test]
async fn reads_and_public_redemption_are_not_rate_limited() {
    // §4 scopes the limit to the two write paths. Throttling the public
    // redemption GET would break the auditor's path at exactly the wrong
    // moment, so prove reads stay open well past the write allowance.
    const LIMIT: u32 = 1;
    let h = Harness::with_rate_limit(LIMIT).await;
    let app = h.app();

    for i in 0..(LIMIT + 5) {
        let req = Request::builder()
            .method("GET")
            .uri("/v1/attestations")
            .header("x-api-key", &h.api_key)
            .body(Body::empty())
            .unwrap();
        let resp = send(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "search request {i} must not be rate limited"
        );
    }

    // The public share-redemption GET is likewise unthrottled (a bogus token
    // 404s, which is fine — what matters is that it is never a 429).
    for i in 0..(LIMIT + 5) {
        let req = Request::builder()
            .method("GET")
            .uri("/v1/share-links/no-such-token")
            .body(Body::empty())
            .unwrap();
        let resp = send(&app, req).await;
        assert_ne!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "public redemption request {i} must never be rate limited"
        );
    }
}

#[tokio::test]
async fn the_default_limit_does_not_throttle_ordinary_traffic() {
    // Guards the pilot default against being set so low that the rest of the
    // suite (and a normal SDK client) starts flaking.
    let h = Harness::new().await;
    let app = h.app();
    for i in 0..20 {
        let resp = send(&app, ingest_body(&h, &format!("CUST-D{i}"))).await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "request {i} must fit inside the default limit of {}",
            attestly_cloud::ratelimit::DEFAULT_PER_MINUTE
        );
    }
}
