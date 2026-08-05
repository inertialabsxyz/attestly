//! Shared helpers for `attestly-cloud` integration tests.
//!
//! All tests run against the in-memory `MemDb` + `MemBlobStore` backends so
//! the suite has zero external dependencies. The Postgres impl is exercised
//! by the staging deploy and the `make seed` smoke; observable behaviour is
//! pinned by the trait contract.

use std::sync::Arc;

use attestly_cloud::auth::{generate_api_key, hash_api_key};
use attestly_cloud::blob::memory::MemBlobStore;
use attestly_cloud::store::memory::MemDb;
use attestly_cloud::store::{Account, Db, Plan};
use attestly_cloud::stripe::MockStripeClient;
use attestly_cloud::{router, router_with_rate_limit, AppState, BillingConfig};
use attestly_core::{signing::AgentKeypair, Attestation, AttestationStatus};
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::util::ServiceExt;

pub struct Harness {
    pub state: AppState,
    /// Per-minute rate limit applied to the write routes. Defaults to
    /// [`attestly_cloud::ratelimit::DEFAULT_PER_MINUTE`] so ordinary tests
    /// never trip it; `rate_limit.rs` pins a low value to observe a 429
    /// deterministically. Held here rather than read from the environment so
    /// one test can't perturb another running in parallel.
    // reason: only the rate_limit binary constructs a non-default value.
    #[allow(dead_code)]
    pub rate_limit_per_min: u32,
    // reason: per-test-binary dead-code analysis flags this in binaries
    // that don't read it directly. Other binaries (search.rs, ingest.rs) do.
    #[allow(dead_code)]
    pub account: Account,
    // reason: telemetry.rs and quickstart.rs mint their own keys via signup
    // and never read this default one; other binaries do.
    #[allow(dead_code)]
    pub api_key: String,
    // reason: only the billing test binary downcasts this to inspect calls;
    // other binaries don't touch it.
    #[allow(dead_code)]
    pub stripe: Arc<MockStripeClient>,
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_billing(BillingConfig::for_tests()).await
    }

    // reason: only share_links.rs injects a non-default billing config (to
    // pin the share-link base URL); other test binaries use `new()`.
    #[allow(dead_code)]
    pub async fn with_billing(billing: BillingConfig) -> Self {
        Self::with_billing_and_rate_limit(billing, attestly_cloud::ratelimit::DEFAULT_PER_MINUTE)
            .await
    }

    /// Harness with an explicit per-minute write-route rate limit. Lets
    /// `rate_limit.rs` pin a limit low enough to trip in a handful of
    /// requests without wall-clock sleeps.
    // reason: only the rate_limit binary needs a non-default limit.
    #[allow(dead_code)]
    pub async fn with_rate_limit(per_min: u32) -> Self {
        Self::with_billing_and_rate_limit(BillingConfig::for_tests(), per_min).await
    }

    async fn with_billing_and_rate_limit(billing: BillingConfig, rate_limit_per_min: u32) -> Self {
        let db: Arc<dyn Db> = Arc::new(MemDb::new());
        let blob = Arc::new(MemBlobStore::new());
        let account = db.create_account("test@local", Plan::Team).await.unwrap();
        let cleartext = generate_api_key();
        let phc = hash_api_key(&cleartext).unwrap();
        db.create_api_key(account.id, "default", &phc)
            .await
            .unwrap();
        let stripe = Arc::new(MockStripeClient::new());
        let state = AppState::new(db, blob, stripe.clone(), billing);
        Self {
            state,
            account,
            api_key: cleartext,
            stripe,
            rate_limit_per_min,
        }
    }

    // reason: used by share_links.rs and retrieval_and_tamper.rs, unused by
    // ingest.rs / search.rs / export_and_health.rs — each test binary builds
    // independently so cargo's dead-code lint sees only its own callers.
    #[allow(dead_code)]
    pub async fn second_account(&self) -> (Account, String) {
        let acct = self
            .state
            .db
            .create_account("second@local", Plan::Team)
            .await
            .unwrap();
        let key = generate_api_key();
        let phc = hash_api_key(&key).unwrap();
        self.state
            .db
            .create_api_key(acct.id, "default", &phc)
            .await
            .unwrap();
        (acct, key)
    }

    /// Send one request through a **freshly built** router.
    ///
    /// Each call constructs its own router, and therefore its own rate-limiter
    /// buckets — so ordinary tests can't accumulate against the limit no
    /// matter how many requests they make. Tests that need the limiter to
    /// actually count across requests must use [`Harness::app`] and hold one
    /// router for the whole sequence.
    // reason: rate_limit.rs drives a persistent router via `app()` instead and
    // never calls this; every other binary does.
    #[allow(dead_code)]
    pub async fn send(&self, req: Request<Body>) -> Response<Body> {
        let app = router(self.state.clone());
        app.oneshot(req).await.unwrap()
    }

    /// Build one long-lived router at this harness's rate limit.
    ///
    /// The limiter's token buckets live in the router, so a caller that wants
    /// to observe throttling must reuse a single instance across requests
    /// (clone it per `oneshot` — `oneshot` consumes the service, but a clone
    /// shares the same buckets).
    // reason: only the rate_limit binary needs a persistent router.
    #[allow(dead_code)]
    pub fn app(&self) -> axum::Router {
        router_with_rate_limit(self.state.clone(), self.rate_limit_per_min)
    }
}

// reason: every binary except telemetry.rs (which never ingests a signed
// attestation) calls this helper.
#[allow(dead_code)]
pub fn make_signed_attestation(
    kp: &AgentKeypair,
    agent_id: &str,
    customer_id: &str,
) -> Attestation {
    let task_hash = attestly_core::signing::sha256(format!("task-{customer_id}").as_bytes());
    let output = serde_json::json!({"customer_id": customer_id, "decision": "Approve"}).to_string();
    let mut a = Attestation::new(
        agent_id,
        task_hash,
        output,
        AttestationStatus::Completed,
        None,
        1_700_000_000,
    );
    kp.sign_attestation(&mut a);
    a
}

pub async fn body_json(resp: Response<Body>) -> (StatusCode, Value) {
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response was not JSON")
    };
    (parts.status, v)
}

// reason: only the share_links.rs binary calls this (HTML response body);
// the JSON-handling binaries don't.
#[allow(dead_code)]
pub async fn body_text(resp: Response<Body>) -> (StatusCode, String) {
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (parts.status, String::from_utf8_lossy(&bytes).to_string())
}

// reason: telemetry.rs uses its own un-authenticated post helper; every
// other binary calls this one.
#[allow(dead_code)]
pub fn post_json(uri: &str, api_key: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("x-api-key", api_key)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// reason: used by every binary except export_and_health.rs (which only
// hits /healthz and the unauthenticated billing-webhook stub).
#[allow(dead_code)]
pub fn get_authed(uri: &str, api_key: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("x-api-key", api_key)
        .body(Body::empty())
        .unwrap()
}

// reason: used by share_links.rs and export_and_health.rs for the public
// surface; binaries that only test the authed routes don't call it.
#[allow(dead_code)]
pub fn get_public(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}
