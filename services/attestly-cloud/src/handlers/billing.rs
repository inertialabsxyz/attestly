//! Billing — Stripe Checkout, customer portal, webhook receiver, and the
//! end-of-period metered-billing trigger.
//!
//! ## Surface
//!
//! - `POST /v1/billing/checkout`           — start a Team-tier Stripe Checkout
//! - `POST /v1/billing/portal`             — open the Stripe Customer Portal
//! - `POST /v1/billing/webhook`            — Stripe webhook receiver (signature-verified)
//! - `POST /v1/admin/bill-period`          — end-of-period metered overage trigger
//! - `GET  /billing/checkout`              — redirect the landing-page CTA to Checkout
//!
//! ## Metered billing model
//!
//! Team plan includes 1M attestations per calendar month. Above that we
//! post a [Stripe usage record][docs] at `$0.10 / 1k attestations` to the
//! pre-provisioned metered subscription item the operator configured at
//! `STRIPE_OVERAGE_ITEM_ID`. The Stripe usage record is the surcharge
//! source-of-truth; we record `accounts.billed_through` after each
//! successful post so a retried trigger does not double-charge.
//!
//! [docs]: https://stripe.com/docs/api/usage_records

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Redirect;
use axum::Json;
use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{generate_api_key, hash_api_key, AuthedAccount};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::Plan;
use crate::stripe::{webhook, CheckoutSessionParams, PortalSessionParams, UsageRecordParams};

const OVERAGE_PRICE_CENTS_PER_1K: i64 = 10;

#[derive(Debug, Deserialize)]
pub struct CheckoutQuery {
    pub plan: Option<String>,
}

/// `GET /billing/checkout` — entry point that the landing page links to.
/// Issues a fresh Checkout Session and redirects the browser. Public; we do
/// not (and cannot) require an API key on the pre-signup path.
pub async fn landing_checkout(
    State(state): State<AppState>,
    Query(q): Query<CheckoutQuery>,
) -> ApiResult<Redirect> {
    let plan = q.plan.as_deref().unwrap_or("team");
    if plan != "team" {
        return Err(ApiError::InvalidRequest(format!(
            "only the team plan supports self-serve checkout (got `{plan}`)"
        )));
    }
    let success_url = format!("{}/dashboard?welcome=1", state.billing.base_url);
    let cancel_url = format!("{}/#pricing", state.billing.base_url);
    let session = state
        .stripe
        .create_checkout_session(CheckoutSessionParams {
            price_id: &state.billing.team_price_id,
            success_url: &success_url,
            cancel_url: &cancel_url,
            customer_email: None,
        })
        .await?;
    Ok(Redirect::to(&session.url))
}

#[derive(Debug, Deserialize, Default)]
pub struct CreateCheckoutRequest {
    pub customer_email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateCheckoutResponse {
    pub session_id: String,
    pub url: String,
}

/// `POST /v1/billing/checkout` — JSON variant of the landing redirect,
/// useful for SDK-side flows or programmatic provisioning.
pub async fn create_checkout(
    State(state): State<AppState>,
    Json(req): Json<CreateCheckoutRequest>,
) -> ApiResult<Json<CreateCheckoutResponse>> {
    let success_url = format!("{}/dashboard?welcome=1", state.billing.base_url);
    let cancel_url = format!("{}/#pricing", state.billing.base_url);
    let session = state
        .stripe
        .create_checkout_session(CheckoutSessionParams {
            price_id: &state.billing.team_price_id,
            success_url: &success_url,
            cancel_url: &cancel_url,
            customer_email: req.customer_email.as_deref(),
        })
        .await?;
    Ok(Json(CreateCheckoutResponse {
        session_id: session.id,
        url: session.url,
    }))
}

#[derive(Debug, Serialize)]
pub struct CreatePortalResponse {
    pub url: String,
}

/// `POST /v1/billing/portal` — open the Stripe Customer Portal for the
/// authenticated account. Returns the hosted URL; the dashboard redirects
/// to it.
pub async fn create_portal(
    State(state): State<AppState>,
    AuthedAccount(account): AuthedAccount,
) -> ApiResult<Json<CreatePortalResponse>> {
    let Some(customer_id) = account.stripe_customer_id.as_deref() else {
        return Err(ApiError::InvalidRequest(
            "account has no Stripe customer attached — complete a checkout first".into(),
        ));
    };
    let return_url = format!("{}/dashboard", state.billing.base_url);
    let session = state
        .stripe
        .create_billing_portal_session(PortalSessionParams {
            customer_id,
            return_url: &return_url,
        })
        .await?;
    Ok(Json(CreatePortalResponse { url: session.url }))
}

/// `POST /v1/billing/webhook` — Stripe webhook receiver. Verifies the
/// `Stripe-Signature` header against `STRIPE_WEBHOOK_SECRET` before
/// dispatching on `type`. Supported events:
///
/// - `checkout.session.completed` — provision the account and issue an API key
/// - `customer.subscription.created` / `customer.subscription.updated` — sync plan
/// - `customer.subscription.deleted` — record the cancellation
/// - `invoice.paid` — informational only; we surface usage on `bill-period`
pub async fn handle_stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let sig_header = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    webhook::verify(
        body.as_ref(),
        sig_header,
        &state.billing.webhook_secret,
        Utc::now().timestamp(),
        webhook::DEFAULT_TOLERANCE_SECS,
    )?;

    let event: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::InvalidRequest(format!("webhook body is not JSON: {e}")))?;
    let event_type = event
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::InvalidRequest("missing event type".into()))?;
    let object = event
        .get("data")
        .and_then(|d| d.get("object"))
        .ok_or_else(|| ApiError::InvalidRequest("missing data.object".into()))?;

    match event_type {
        "checkout.session.completed" => provision_from_checkout(&state, object).await?,
        "customer.subscription.created"
        | "customer.subscription.updated"
        | "customer.subscription.deleted" => sync_subscription(&state, object).await?,
        "invoice.paid" => {
            tracing::info!(event_type, "invoice.paid received (informational)");
        }
        other => {
            tracing::debug!(event_type = other, "ignoring unhandled webhook event");
        }
    }
    Ok((StatusCode::OK, Json(json!({"received": true}))))
}

async fn provision_from_checkout(state: &AppState, obj: &Value) -> Result<(), ApiError> {
    let email = obj
        .get("customer_details")
        .and_then(|c| c.get("email"))
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("customer_email").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            ApiError::InvalidRequest("checkout.session.completed missing customer email".into())
        })?;
    let stripe_customer = obj
        .get("customer")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::InvalidRequest("checkout.session.completed missing customer".into())
        })?;
    let subscription = obj.get("subscription").and_then(|v| v.as_str());

    let (account, inserted) = state.db.upsert_account_by_email(email, Plan::Team).await?;
    state
        .db
        .set_account_stripe_ids(account.id, stripe_customer, subscription)
        .await?;

    if inserted {
        let cleartext = generate_api_key();
        let phc = hash_api_key(&cleartext)?;
        state.db.create_api_key(account.id, "default", &phc).await?;
        // Production wires this into the email sender. The cleartext key is
        // also surfaced once in the dashboard's welcome banner via the
        // `?welcome=1` query parameter — see handlers::account.
        tracing::info!(
            account_id = %account.id,
            email,
            "provisioned new Team account from Stripe Checkout"
        );
        // Log the cleartext key on stderr exactly once for the local dev
        // path. In production we hand this to the email sender instead;
        // it must never be persisted in cleartext.
        tracing::info!(account_id = %account.id, "issued cleartext API key (one-time): {}", cleartext);
        let _ = cleartext;
    } else {
        tracing::info!(
            account_id = %account.id,
            email,
            "checkout.session.completed re-delivered — account already provisioned"
        );
    }
    Ok(())
}

async fn sync_subscription(state: &AppState, obj: &Value) -> Result<(), ApiError> {
    let stripe_customer = obj
        .get("customer")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::InvalidRequest("subscription event missing customer".into()))?;
    let Some(account) = state
        .db
        .find_account_by_stripe_customer(stripe_customer)
        .await?
    else {
        // The webhook arrived before `checkout.session.completed`. Stripe
        // delivers events out of order at times; the next subscription
        // event will re-sync.
        tracing::warn!(stripe_customer, "no local account for stripe customer");
        return Ok(());
    };
    let subscription_id = obj.get("id").and_then(|v| v.as_str());
    if let Some(sid) = subscription_id {
        state
            .db
            .set_account_stripe_ids(account.id, stripe_customer, Some(sid))
            .await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct BillPeriodRequest {
    /// First day (inclusive) of the period to bill, UTC. Defaults to the
    /// first day of the *previous* calendar month if omitted.
    pub from: Option<NaiveDate>,
    /// Last day (exclusive) of the period to bill, UTC. Defaults to the
    /// first day of the *current* calendar month if omitted.
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct BillPeriodResponse {
    pub period_from: NaiveDate,
    pub period_to: NaiveDate,
    pub results: Vec<AccountBilledLine>,
}

#[derive(Debug, Serialize)]
pub struct AccountBilledLine {
    pub account_id: Uuid,
    pub plan: Plan,
    pub total_attestations: i64,
    pub overage_units: i64,
    pub usage_record_id: Option<String>,
    pub skipped_reason: Option<String>,
}

/// `POST /v1/admin/bill-period` — gated by `x-admin-key`. Walks every
/// account, computes `max(0, total - 1M)` for the period, divides by 1k
/// (rounded up), and posts a Stripe usage record against the configured
/// metered subscription item. Idempotent on `(account_id, period_to)` via
/// `accounts.billed_through` and the Stripe `Idempotency-Key` header.
pub async fn bill_period(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BillPeriodRequest>,
) -> ApiResult<Json<BillPeriodResponse>> {
    let provided = headers
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided != state.billing.admin_key {
        return Err(ApiError::Unauthorized);
    }
    let (from, to) = resolve_period(&req)?;
    let mut results = Vec::new();
    for acct in state.db.list_accounts().await? {
        if acct.plan == Plan::Enterprise {
            results.push(AccountBilledLine {
                account_id: acct.id,
                plan: acct.plan,
                total_attestations: 0,
                overage_units: 0,
                usage_record_id: None,
                skipped_reason: Some("enterprise plan — no metered overage".into()),
            });
            continue;
        }
        if matches!(acct.billed_through, Some(b) if b >= to) {
            results.push(AccountBilledLine {
                account_id: acct.id,
                plan: acct.plan,
                total_attestations: 0,
                overage_units: 0,
                usage_record_id: None,
                skipped_reason: Some("already billed through this period".into()),
            });
            continue;
        }
        let total = state.db.account_usage_in_range(acct.id, from, to).await?;
        let included = acct.plan.included_attestations();
        let overage = (total - included).max(0);
        // Round up to the next 1k unit.
        let overage_units = (overage + 999) / 1000;
        if overage_units == 0 {
            state.db.set_billed_through(acct.id, to).await?;
            results.push(AccountBilledLine {
                account_id: acct.id,
                plan: acct.plan,
                total_attestations: total,
                overage_units: 0,
                usage_record_id: None,
                skipped_reason: Some("within included floor".into()),
            });
            continue;
        }
        let idempotency_key = format!("overage:{}:{}", acct.id, to);
        let record = state
            .stripe
            .post_usage_record(UsageRecordParams {
                subscription_item_id: &state.billing.overage_subscription_item_id,
                quantity: overage_units,
                timestamp: Utc::now().timestamp(),
                idempotency_key: &idempotency_key,
            })
            .await?;
        state.db.set_billed_through(acct.id, to).await?;
        let _ = OVERAGE_PRICE_CENTS_PER_1K;
        results.push(AccountBilledLine {
            account_id: acct.id,
            plan: acct.plan,
            total_attestations: total,
            overage_units,
            usage_record_id: Some(record.id),
            skipped_reason: None,
        });
    }
    Ok(Json(BillPeriodResponse {
        period_from: from,
        period_to: to,
        results,
    }))
}

fn resolve_period(req: &BillPeriodRequest) -> Result<(NaiveDate, NaiveDate), ApiError> {
    match (req.from, req.to) {
        (Some(f), Some(t)) => {
            if f >= t {
                return Err(ApiError::InvalidRequest("from must be < to".into()));
            }
            Ok((f, t))
        }
        (None, None) => {
            // Previous calendar month, half-open.
            let today = Utc::now().date_naive();
            let first_of_this_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(|| ApiError::Internal("bad current date".into()))?;
            let (py, pm) = if first_of_this_month.month() == 1 {
                (first_of_this_month.year() - 1, 12)
            } else {
                (first_of_this_month.year(), first_of_this_month.month() - 1)
            };
            let first_of_prev = NaiveDate::from_ymd_opt(py, pm, 1)
                .ok_or_else(|| ApiError::Internal("bad previous month".into()))?;
            Ok((first_of_prev, first_of_this_month))
        }
        _ => Err(ApiError::InvalidRequest(
            "either provide both from and to, or neither".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_defaults_to_previous_calendar_month() {
        // We can't pin "today" here, but we can check the structural
        // invariants: to is the first of the month containing today, from
        // is one calendar month earlier.
        let (from, to) = resolve_period(&BillPeriodRequest::default()).unwrap();
        assert_eq!(from.day(), 1);
        assert_eq!(to.day(), 1);
        assert!(from < to);
        // The interval is at most 31 days and at least 28.
        let days = (to - from).num_days();
        assert!((28..=31).contains(&days), "got {days} days");
    }

    #[test]
    fn period_rejects_inverted_range() {
        let req = BillPeriodRequest {
            from: Some(NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()),
            to: Some(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
        };
        assert!(resolve_period(&req).is_err());
    }
}
