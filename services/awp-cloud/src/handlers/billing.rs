//! `POST /v1/billing/webhook` — Stripe webhook receiver. **Stub for Step 2.**
//!
//! This file exists so Step 4 (pricing & billing) can fill in the handler
//! without changing the routing layer of `awp-cloud`. The route is locked in
//! by `crate::router`; the implementation here returns
//! `501 Not Implemented` until Step 4 wires it to Stripe's signing webhook.
//!
//! Step 4 contract:
//! - Validate `Stripe-Signature` header against `STRIPE_WEBHOOK_SECRET`
//! - Dispatch on the event type (`invoice.paid`, `customer.subscription.*`)
//! - Update `accounts.stripe_customer_id` / retention / plan on events
//! - Emit metered-billing records from the `usage` table on month boundaries

use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

/// Step 4 fills this in. **Do not** change the signature or path — the
/// routing layer is locked.
pub async fn handle_stripe_webhook(_body: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "not_implemented",
            "detail": "stripe webhook lands in gtm phase 2 step 4"
        })),
    )
}
