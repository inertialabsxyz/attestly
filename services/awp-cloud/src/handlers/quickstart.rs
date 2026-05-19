//! Quickstart and self-serve signup handlers.
//!
//! Surfaces:
//!
//! - `GET  /quickstart`          — server-rendered HTML page that walks a new
//!   user through signup → `pip install` → 5-line snippet → first attestation.
//! - `POST /v1/account/signup`   — create a new Team account from email +
//!   password, mint the first API key, return the cleartext key exactly once.
//!
//! The signup path is deliberately Team-tier only. Stripe Checkout (Step 4)
//! remains the path for users who want to attach a real subscription up
//! front; this is the free-trial / kick-the-tires path for the quickstart.
//! The 1M-attestations/month Team floor means a quickstart account never
//! incurs overage during the trial.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{generate_api_key, hash_api_key, hash_password};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::Plan;

const MIN_PASSWORD_LEN: usize = 8;
const DEFAULT_PROJECT_NAME: &str = "quickstart";

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct SignupResponse {
    pub account_id: Uuid,
    pub email: String,
    pub plan: Plan,
    /// The cleartext API key, returned exactly once. The quickstart page
    /// writes this into `localStorage` so the dashboard can authenticate
    /// without a second round trip.
    pub api_key: String,
    pub dashboard_url: String,
}

/// `POST /v1/account/signup` — create a new Team account.
pub async fn signup(
    State(state): State<AppState>,
    Json(req): Json<SignupRequest>,
) -> ApiResult<(StatusCode, Json<SignupResponse>)> {
    let email = req.email.trim().to_ascii_lowercase();
    if !is_plausible_email(&email) {
        return Err(ApiError::InvalidRequest("invalid email address".into()));
    }
    if req.password.len() < MIN_PASSWORD_LEN {
        return Err(ApiError::InvalidRequest(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }

    if state.db.find_account_by_email(&email).await?.is_some() {
        return Err(ApiError::InvalidRequest(
            "an account with that email already exists".into(),
        ));
    }

    let password_hash = hash_password(&req.password)?;
    let account = state
        .db
        .create_account_with_password(&email, &password_hash, Plan::Team)
        .await?;

    let cleartext = generate_api_key();
    let key_phc = hash_api_key(&cleartext)?;
    state
        .db
        .create_api_key(account.id, DEFAULT_PROJECT_NAME, &key_phc)
        .await?;

    let dashboard_url = format!("{}/dashboard?welcome=1", state.billing.base_url);
    Ok((
        StatusCode::CREATED,
        Json(SignupResponse {
            account_id: account.id,
            email: account.email,
            plan: account.plan,
            api_key: cleartext,
            dashboard_url,
        }),
    ))
}

/// `GET /quickstart` — server-rendered 60-second flow.
pub async fn render_quickstart(State(state): State<AppState>) -> Html<String> {
    Html(crate::web::quickstart_html(&state.billing.base_url))
}

fn is_plausible_email(s: &str) -> bool {
    let at = s.find('@');
    match at {
        None => false,
        Some(i) => i > 0 && i < s.len() - 1 && s[i + 1..].contains('.'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_email_accepts_normal_addresses() {
        assert!(is_plausible_email("user@example.com"));
        assert!(is_plausible_email("u+tag@sub.example.co"));
    }

    #[test]
    fn plausible_email_rejects_missing_parts() {
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@example.com"));
        assert!(!is_plausible_email("user@"));
        assert!(!is_plausible_email("user@nodot"));
    }
}
