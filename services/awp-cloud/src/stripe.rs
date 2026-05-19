//! Thin Stripe client. We only need a handful of calls — Checkout Session
//! create, Customer Portal Session create, and metered usage-record post —
//! so we wrap them by hand instead of pulling in `async-stripe` (heavy,
//! churning API surface). Webhook signature verification is implemented in
//! [`webhook`].
//!
//! The trait/impl split keeps the handlers testable: production wires up
//! [`LiveStripeClient`] against `https://api.stripe.com/v1/`; the test
//! suite injects a [`MockStripeClient`] that records every call and
//! deterministically returns canned ids.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::ApiError;

/// Stripe API surface we depend on. Keep this trait minimal — every method
/// here is a surface we will have to mock in tests.
#[async_trait]
pub trait StripeClient: Send + Sync + 'static {
    /// Create a Checkout Session for the Team-tier price. Returns the hosted
    /// URL the caller should redirect to.
    async fn create_checkout_session(
        &self,
        params: CheckoutSessionParams<'_>,
    ) -> Result<CheckoutSession, ApiError>;

    /// Create a Customer Portal Session for an existing Stripe customer.
    async fn create_billing_portal_session(
        &self,
        params: PortalSessionParams<'_>,
    ) -> Result<PortalSession, ApiError>;

    /// Post a metered-billing usage record against a subscription item.
    /// Stripe is the source of truth for the per-period charge; we just hand
    /// it the count. `idempotency_key` is the surcharge gate against retry
    /// (Stripe dedupes on this header for 24h).
    async fn post_usage_record(
        &self,
        params: UsageRecordParams<'_>,
    ) -> Result<UsageRecord, ApiError>;
}

#[derive(Debug, Clone)]
pub struct CheckoutSessionParams<'a> {
    pub price_id: &'a str,
    pub success_url: &'a str,
    pub cancel_url: &'a str,
    /// Embedded in `metadata.account_email` so the webhook can resolve it
    /// back to an account; Stripe doesn't carry our domain model.
    pub customer_email: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PortalSessionParams<'a> {
    pub customer_id: &'a str,
    pub return_url: &'a str,
}

#[derive(Debug, Clone)]
pub struct PortalSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct UsageRecordParams<'a> {
    pub subscription_item_id: &'a str,
    pub quantity: i64,
    pub timestamp: i64,
    pub idempotency_key: &'a str,
}

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub id: String,
    pub quantity: i64,
}

/// Production Stripe client. `reqwest`-based; reads the API key from the
/// `STRIPE_API_KEY` env at construction time so config errors surface at
/// boot.
pub struct LiveStripeClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl LiveStripeClient {
    pub fn from_env() -> Result<Self, ApiError> {
        let api_key = std::env::var("STRIPE_API_KEY")
            .map_err(|_| ApiError::Internal("STRIPE_API_KEY must be set".into()))?;
        let base_url = std::env::var("STRIPE_API_BASE")
            .unwrap_or_else(|_| "https://api.stripe.com/v1".to_string());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ApiError::Internal(format!("reqwest build: {e}")))?;
        Ok(Self {
            api_key,
            base_url,
            http,
        })
    }

    async fn post_form<R>(
        &self,
        path: &str,
        params: &[(&str, &str)],
        idempotency_key: Option<&str>,
    ) -> Result<R, ApiError>
    where
        R: serde::de::DeserializeOwned,
    {
        let url = format!("{}{path}", self.base_url);
        let mut req = self.http.post(&url).basic_auth(&self.api_key, Some(""));
        if let Some(k) = idempotency_key {
            req = req.header("Idempotency-Key", k);
        }
        let resp = req
            .form(params)
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("stripe http: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ApiError::Internal(format!("stripe body: {e}")))?;
        if !status.is_success() {
            return Err(ApiError::Internal(format!(
                "stripe {path} returned {status}: {body}"
            )));
        }
        serde_json::from_str::<R>(&body)
            .map_err(|e| ApiError::Internal(format!("stripe parse {path}: {e}")))
    }
}

#[async_trait]
impl StripeClient for LiveStripeClient {
    async fn create_checkout_session(
        &self,
        params: CheckoutSessionParams<'_>,
    ) -> Result<CheckoutSession, ApiError> {
        let mut form: Vec<(&str, &str)> = vec![
            ("mode", "subscription"),
            ("line_items[0][price]", params.price_id),
            ("line_items[0][quantity]", "1"),
            ("success_url", params.success_url),
            ("cancel_url", params.cancel_url),
            ("allow_promotion_codes", "true"),
        ];
        if let Some(email) = params.customer_email {
            form.push(("customer_email", email));
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            id: String,
            url: Option<String>,
        }
        let r: Resp = self.post_form("/checkout/sessions", &form, None).await?;
        Ok(CheckoutSession {
            id: r.id,
            url: r.url.unwrap_or_default(),
        })
    }

    async fn create_billing_portal_session(
        &self,
        params: PortalSessionParams<'_>,
    ) -> Result<PortalSession, ApiError> {
        let form: Vec<(&str, &str)> = vec![
            ("customer", params.customer_id),
            ("return_url", params.return_url),
        ];
        #[derive(serde::Deserialize)]
        struct Resp {
            id: String,
            url: String,
        }
        let r: Resp = self
            .post_form("/billing_portal/sessions", &form, None)
            .await?;
        Ok(PortalSession {
            id: r.id,
            url: r.url,
        })
    }

    async fn post_usage_record(
        &self,
        params: UsageRecordParams<'_>,
    ) -> Result<UsageRecord, ApiError> {
        let quantity_s = params.quantity.to_string();
        let ts_s = params.timestamp.to_string();
        let form: Vec<(&str, &str)> = vec![
            ("quantity", &quantity_s),
            ("timestamp", &ts_s),
            ("action", "set"),
        ];
        let path = format!(
            "/subscription_items/{}/usage_records",
            params.subscription_item_id
        );
        #[derive(serde::Deserialize)]
        struct Resp {
            id: String,
            quantity: i64,
        }
        let r: Resp = self
            .post_form(&path, &form, Some(params.idempotency_key))
            .await?;
        Ok(UsageRecord {
            id: r.id,
            quantity: r.quantity,
        })
    }
}

/// Test double. Records every call so assertions can inspect what was
/// requested; returns deterministic ids so the surrounding code path is
/// fully observable.
#[derive(Default)]
pub struct MockStripeClient {
    calls: Mutex<Calls>,
}

#[derive(Default)]
struct Calls {
    checkouts: Vec<RecordedCheckout>,
    portals: Vec<RecordedPortal>,
    usage_records: Vec<RecordedUsage>,
    idempotency_keys: HashMap<String, UsageRecord>,
}

#[derive(Debug, Clone)]
pub struct RecordedCheckout {
    pub price_id: String,
    pub customer_email: Option<String>,
    pub success_url: String,
    pub cancel_url: String,
}

#[derive(Debug, Clone)]
pub struct RecordedPortal {
    pub customer_id: String,
    pub return_url: String,
}

#[derive(Debug, Clone)]
pub struct RecordedUsage {
    pub subscription_item_id: String,
    pub quantity: i64,
    pub timestamp: i64,
    pub idempotency_key: String,
}

impl MockStripeClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checkouts(&self) -> Vec<RecordedCheckout> {
        self.calls.lock().unwrap().checkouts.clone()
    }
    pub fn portals(&self) -> Vec<RecordedPortal> {
        self.calls.lock().unwrap().portals.clone()
    }
    pub fn usage_records(&self) -> Vec<RecordedUsage> {
        self.calls.lock().unwrap().usage_records.clone()
    }
}

#[async_trait]
impl StripeClient for MockStripeClient {
    async fn create_checkout_session(
        &self,
        params: CheckoutSessionParams<'_>,
    ) -> Result<CheckoutSession, ApiError> {
        let mut calls = self.calls.lock().unwrap();
        let n = calls.checkouts.len();
        calls.checkouts.push(RecordedCheckout {
            price_id: params.price_id.to_string(),
            customer_email: params.customer_email.map(str::to_string),
            success_url: params.success_url.to_string(),
            cancel_url: params.cancel_url.to_string(),
        });
        let id = format!("cs_test_{n:08}");
        Ok(CheckoutSession {
            url: format!("https://stripe.test/checkout/{id}"),
            id,
        })
    }

    async fn create_billing_portal_session(
        &self,
        params: PortalSessionParams<'_>,
    ) -> Result<PortalSession, ApiError> {
        let mut calls = self.calls.lock().unwrap();
        let n = calls.portals.len();
        calls.portals.push(RecordedPortal {
            customer_id: params.customer_id.to_string(),
            return_url: params.return_url.to_string(),
        });
        let id = format!("bps_test_{n:08}");
        Ok(PortalSession {
            url: format!("https://stripe.test/portal/{id}"),
            id,
        })
    }

    async fn post_usage_record(
        &self,
        params: UsageRecordParams<'_>,
    ) -> Result<UsageRecord, ApiError> {
        let mut calls = self.calls.lock().unwrap();
        if let Some(prior) = calls.idempotency_keys.get(params.idempotency_key) {
            // Replay the prior response without recording the call again —
            // mirrors Stripe's 24h idempotency-key dedupe.
            return Ok(prior.clone());
        }
        let n = calls.usage_records.len();
        calls.usage_records.push(RecordedUsage {
            subscription_item_id: params.subscription_item_id.to_string(),
            quantity: params.quantity,
            timestamp: params.timestamp,
            idempotency_key: params.idempotency_key.to_string(),
        });
        let rec = UsageRecord {
            id: format!("mbur_test_{n:08}"),
            quantity: params.quantity,
        };
        calls
            .idempotency_keys
            .insert(params.idempotency_key.to_string(), rec.clone());
        Ok(rec)
    }
}

pub mod webhook {
    //! Verify Stripe's `Stripe-Signature` header against the webhook signing
    //! secret. We only support v1 signatures; older versions are rejected.
    //!
    //! The 5-minute tolerance window matches Stripe's recommended default
    //! and is what protects against replay attacks of an old (valid) payload.

    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use subtle::ConstantTimeEq;

    use crate::error::ApiError;

    /// Default replay window — Stripe's recommended value.
    pub const DEFAULT_TOLERANCE_SECS: i64 = 300;

    /// Parse the `Stripe-Signature` header and verify it matches `payload`
    /// signed by `secret`. Returns the timestamp from the header on success
    /// so the caller can replay-check it if it wants something tighter than
    /// the default tolerance.
    pub fn verify(
        payload: &[u8],
        header: &str,
        secret: &str,
        now_unix: i64,
        tolerance_secs: i64,
    ) -> Result<i64, ApiError> {
        let mut timestamp: Option<i64> = None;
        let mut signatures: Vec<Vec<u8>> = Vec::new();
        for part in header.split(',') {
            let mut it = part.splitn(2, '=');
            let key = it.next().unwrap_or("");
            let val = it.next().unwrap_or("");
            match key.trim() {
                "t" => {
                    timestamp = val.trim().parse::<i64>().ok();
                }
                "v1" => {
                    if let Ok(bytes) = hex::decode(val.trim()) {
                        signatures.push(bytes);
                    }
                }
                _ => {}
            }
        }
        let ts = timestamp.ok_or_else(|| {
            ApiError::InvalidRequest("Stripe-Signature missing `t=` field".into())
        })?;
        if signatures.is_empty() {
            return Err(ApiError::InvalidRequest(
                "Stripe-Signature has no `v1=` signature".into(),
            ));
        }
        // Replay window
        if (now_unix - ts).abs() > tolerance_secs {
            return Err(ApiError::InvalidRequest(
                "Stripe-Signature timestamp outside tolerance window".into(),
            ));
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| ApiError::Internal(format!("hmac key: {e}")))?;
        mac.update(ts.to_string().as_bytes());
        mac.update(b".");
        mac.update(payload);
        let expected = mac.finalize().into_bytes();
        let any_match = signatures
            .iter()
            .any(|sig| sig.len() == expected.len() && sig.ct_eq(&expected[..]).into());
        if !any_match {
            return Err(ApiError::Unauthorized);
        }
        Ok(ts)
    }

    /// Sign `payload` the way Stripe would, for test fixtures. Production
    /// servers never sign incoming webhooks themselves — Stripe does — so
    /// this function only ever runs from the test suite. It is `pub` so
    /// integration tests in `tests/` can call it; gating with `#[cfg(test)]`
    /// would scope it to the lib's unit tests only.
    pub fn sign_for_test(payload: &[u8], secret: &str, timestamp: i64) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(payload);
        let bytes = mac.finalize().into_bytes();
        format!("t={timestamp},v1={}", hex::encode(bytes))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips() {
            let secret = "whsec_test";
            let payload = br#"{"id":"evt_1","type":"invoice.paid"}"#;
            let ts = 1_700_000_000i64;
            let header = sign_for_test(payload, secret, ts);
            assert_eq!(verify(payload, &header, secret, ts, 60).unwrap(), ts);
        }

        #[test]
        fn rejects_modified_payload() {
            let secret = "whsec_test";
            let payload = b"hello";
            let ts = 1_700_000_000i64;
            let header = sign_for_test(payload, secret, ts);
            assert!(matches!(
                verify(b"hello-tampered", &header, secret, ts, 60),
                Err(ApiError::Unauthorized)
            ));
        }

        #[test]
        fn rejects_wrong_secret() {
            let header = sign_for_test(b"x", "right", 1_700_000_000);
            assert!(matches!(
                verify(b"x", &header, "wrong", 1_700_000_000, 60),
                Err(ApiError::Unauthorized)
            ));
        }

        #[test]
        fn rejects_stale_timestamp() {
            let secret = "whsec_test";
            let ts = 1_700_000_000i64;
            let header = sign_for_test(b"x", secret, ts);
            // Pretend "now" is an hour later; default 5-minute window must
            // refuse.
            let err = verify(b"x", &header, secret, ts + 3600, 300).unwrap_err();
            assert!(matches!(err, ApiError::InvalidRequest(_)));
        }
    }
}
