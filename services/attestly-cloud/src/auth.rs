//! API-key authentication.
//!
//! ## How keys travel
//!
//! - Clients hold a 36-char UUID. They send it in the `x-api-key` header.
//! - The server **never** stores the cleartext key. We store an Argon2id PHC
//!   string in `api_keys.hashed_key`.
//! - On every request we fetch the candidate set of active hashes (in
//!   practice this is per-account once we have a key prefix on the wire,
//!   but for v0.1 we scan; it's a small table) and run Argon2 against each.
//!
//! ## Why per-row scan
//!
//! Argon2 verification is single-key — there's no SQL predicate that says
//! "this UUID hashes to this PHC string." So either:
//!
//! 1. Scan, verify each — fine for hundreds of keys
//! 2. Add a fast non-cryptographic lookup column (e.g. SHA-256 of the key)
//!    plus Argon2 for the actual check
//!
//! v0.1 takes option 1. Option 2 is a Phase-3 perf upgrade.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;
use crate::store::Account;

/// Argon2id hash a cleartext API key. The resulting PHC string is what we
/// persist in `api_keys.hashed_key`. Salt is per-key.
pub fn hash_api_key(cleartext: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::default();
    let hash = argon
        .hash_password(cleartext.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(format!("argon2 hash: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a cleartext key against a stored Argon2 PHC string. `false` on any
/// failure — we deliberately do not distinguish "wrong key" from "bad hash."
pub fn verify_api_key(cleartext: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(cleartext.as_bytes(), &parsed)
        .is_ok()
}

/// Argon2id hash a cleartext signup password. Same algorithm and parameters
/// as [`hash_api_key`]; the two are kept as separate helpers only for call-
/// site clarity.
pub fn hash_password(cleartext: &str) -> Result<String, ApiError> {
    hash_api_key(cleartext)
}

/// Resolve an `x-api-key` header against the database, returning the owning
/// `Account` on success. `None` on missing header or no match (handlers
/// distinguish these only insofar as "missing header" returns 401 with
/// `unauthorized`; both paths are surfaced the same).
async fn authenticate(headers: &HeaderMap, state: &AppState) -> Result<Account, ApiError> {
    let raw = headers
        .get("x-api-key")
        .ok_or(ApiError::Unauthorized)?
        .to_str()
        .map_err(|_| ApiError::Unauthorized)?;
    if raw.trim().is_empty() {
        return Err(ApiError::Unauthorized);
    }
    let candidates = state.db.list_active_hashed_keys().await?;
    for (phc, account_id) in candidates {
        if verify_api_key(raw, &phc) {
            // Re-fetch via the indexed lookup so the response carries the
            // canonical account row (email, retention, etc.).
            let Some(account) = state.db.find_account_by_hashed_key(&phc).await? else {
                continue;
            };
            debug_assert_eq!(account.id, account_id);
            return Ok(account);
        }
    }
    Err(ApiError::Unauthorized)
}

/// Axum extractor that resolves the bearer to an `Account`. Handlers that
/// take `AuthedAccount` are automatically gated on a valid `x-api-key`.
pub struct AuthedAccount(pub Account);

impl AuthedAccount {
    pub fn id(&self) -> Uuid {
        self.0.id
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthedAccount {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let account = authenticate(&parts.headers, state).await?;
        Ok(AuthedAccount(account))
    }
}

/// Generate a fresh cleartext API key. Caller is expected to hash it before
/// persisting and to surface the cleartext to the user exactly once.
pub fn generate_api_key() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_round_trips() {
        let key = generate_api_key();
        let phc = hash_api_key(&key).unwrap();
        assert!(verify_api_key(&key, &phc));
    }

    #[test]
    fn wrong_key_does_not_verify() {
        let phc = hash_api_key("k1").unwrap();
        assert!(!verify_api_key("k2", &phc));
    }

    #[test]
    fn malformed_phc_does_not_verify() {
        assert!(!verify_api_key("any", "not-a-phc-string"));
    }
}
