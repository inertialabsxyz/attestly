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
use attestly_cloud::{router, AppState, BillingConfig};
use attestly_core::{signing::AgentKeypair, Attestation, AttestationStatus};
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::util::ServiceExt;

pub struct Harness {
    pub state: AppState,
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

    pub async fn send(&self, req: Request<Body>) -> Response<Body> {
        let app = router(self.state.clone());
        app.oneshot(req).await.unwrap()
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
