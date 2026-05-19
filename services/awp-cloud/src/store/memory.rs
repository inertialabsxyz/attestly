//! Pure-Rust in-memory `Db` impl. Used by the unit/integration test suite and
//! by `cargo run` in CI smoke mode. The Postgres impl in
//! `crate::store::postgres` is the production target — they implement the
//! same trait and must agree on observable behaviour.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::ApiError;
use crate::store::{
    Account, ApiKeyRecord, AttestationIndex, Db, IngestOutcome, SearchFilters, SearchPage,
    ShareLink,
};

#[derive(Default)]
struct Inner {
    accounts: Vec<Account>,
    keys: Vec<ApiKeyRecord>,
    attestations: Vec<AttestationIndex>,
    share_links: Vec<ShareLink>,
    /// `(account_id, yyyy-mm-dd) -> count`.
    usage: BTreeMap<(Uuid, String), i64>,
}

pub struct MemDb {
    inner: Mutex<Inner>,
}

impl MemDb {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }
}

impl Default for MemDb {
    fn default() -> Self {
        Self::new()
    }
}

fn lock_internal<F, R>(inner: &Mutex<Inner>, f: F) -> Result<R, ApiError>
where
    F: FnOnce(&mut Inner) -> R,
{
    let mut guard = inner
        .lock()
        .map_err(|_| ApiError::Internal("mem_db poisoned".into()))?;
    Ok(f(&mut guard))
}

#[async_trait]
impl Db for MemDb {
    async fn ping(&self) -> Result<(), ApiError> {
        Ok(())
    }

    async fn create_account(&self, email: &str) -> Result<Account, ApiError> {
        let acct = Account {
            id: Uuid::new_v4(),
            email: email.to_string(),
            stripe_customer_id: None,
            retention_days: 365,
            created_at: Utc::now(),
        };
        lock_internal(&self.inner, |inner| inner.accounts.push(acct.clone()))?;
        Ok(acct)
    }

    async fn create_api_key(
        &self,
        account_id: Uuid,
        project_name: &str,
        hashed_key: &str,
    ) -> Result<ApiKeyRecord, ApiError> {
        let rec = ApiKeyRecord {
            id: Uuid::new_v4(),
            account_id,
            project_name: project_name.to_string(),
            hashed_key: hashed_key.to_string(),
            created_at: Utc::now(),
            revoked_at: None,
        };
        lock_internal(&self.inner, |inner| inner.keys.push(rec.clone()))?;
        Ok(rec)
    }

    async fn find_account_by_hashed_key(
        &self,
        hashed_key: &str,
    ) -> Result<Option<Account>, ApiError> {
        lock_internal(&self.inner, |inner| {
            let key = inner
                .keys
                .iter()
                .find(|k| k.hashed_key == hashed_key && k.revoked_at.is_none())?;
            let acct = inner.accounts.iter().find(|a| a.id == key.account_id)?;
            Some(acct.clone())
        })
    }

    async fn list_active_hashed_keys(&self) -> Result<Vec<(String, Uuid)>, ApiError> {
        lock_internal(&self.inner, |inner| {
            inner
                .keys
                .iter()
                .filter(|k| k.revoked_at.is_none())
                .map(|k| (k.hashed_key.clone(), k.account_id))
                .collect()
        })
    }

    async fn insert_attestation(&self, index: AttestationIndex) -> Result<IngestOutcome, ApiError> {
        lock_internal(&self.inner, |inner| {
            if let Some(existing) = inner.attestations.iter().find(|a| a.id == index.id) {
                IngestOutcome::Existed(existing.clone())
            } else {
                inner.attestations.push(index.clone());
                IngestOutcome::Inserted(index)
            }
        })
    }

    async fn fetch_attestation(
        &self,
        account_id: Uuid,
        id: Uuid,
    ) -> Result<Option<AttestationIndex>, ApiError> {
        lock_internal(&self.inner, |inner| {
            inner
                .attestations
                .iter()
                .find(|a| a.id == id && a.account_id == account_id)
                .cloned()
        })
    }

    async fn search_attestations(
        &self,
        account_id: Uuid,
        filters: SearchFilters,
    ) -> Result<SearchPage, ApiError> {
        lock_internal(&self.inner, |inner| {
            let mut filtered: Vec<AttestationIndex> = inner
                .attestations
                .iter()
                .filter(|a| a.account_id == account_id)
                .filter(|a| {
                    filters
                        .agent_id
                        .as_ref()
                        .map(|v| &a.agent_id == v)
                        .unwrap_or(true)
                })
                .filter(|a| {
                    filters
                        .customer_id
                        .as_ref()
                        .map(|v| a.customer_id.as_ref() == Some(v))
                        .unwrap_or(true)
                })
                .filter(|a| filters.from.map(|f| a.received_at >= f).unwrap_or(true))
                .filter(|a| filters.to.map(|t| a.received_at < t).unwrap_or(true))
                .cloned()
                .collect();
            // Stable order: newest first.
            filtered.sort_by(|a, b| b.received_at.cmp(&a.received_at).then(b.id.cmp(&a.id)));

            // Cursor is the index into the filtered list. Crude but matches
            // the semantics tests need; the Postgres impl uses a composite
            // key cursor for correctness under concurrent inserts.
            let start: usize = filters
                .cursor
                .as_deref()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let end = (start + filters.limit).min(filtered.len());
            let rows = filtered[start..end].to_vec();
            let next_cursor = if end < filtered.len() {
                Some(end.to_string())
            } else {
                None
            };
            SearchPage { rows, next_cursor }
        })
    }

    async fn list_all_attestations(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<AttestationIndex>, ApiError> {
        lock_internal(&self.inner, |inner| {
            let mut rows: Vec<AttestationIndex> = inner
                .attestations
                .iter()
                .filter(|a| a.account_id == account_id)
                .cloned()
                .collect();
            rows.sort_by(|a, b| a.received_at.cmp(&b.received_at));
            rows
        })
    }

    async fn create_share_link(&self, link: ShareLink) -> Result<(), ApiError> {
        lock_internal(&self.inner, |inner| inner.share_links.push(link))?;
        Ok(())
    }

    async fn fetch_share_link(&self, token: &str) -> Result<Option<ShareLink>, ApiError> {
        lock_internal(&self.inner, |inner| {
            inner.share_links.iter().find(|l| l.token == token).cloned()
        })
    }

    async fn revoke_share_link(&self, account_id: Uuid, token: &str) -> Result<bool, ApiError> {
        lock_internal(&self.inner, |inner| {
            for link in inner.share_links.iter_mut() {
                if link.token == token && link.account_id == account_id {
                    if link.revoked_at.is_none() {
                        link.revoked_at = Some(Utc::now());
                    }
                    return true;
                }
            }
            false
        })
    }

    async fn record_usage(&self, account_id: Uuid, at: DateTime<Utc>) -> Result<(), ApiError> {
        let day = at.format("%Y-%m-%d").to_string();
        lock_internal(&self.inner, |inner| {
            *inner.usage.entry((account_id, day)).or_insert(0) += 1;
        })
    }

    async fn account_usage_total(&self, account_id: Uuid) -> Result<i64, ApiError> {
        lock_internal(&self.inner, |inner| {
            inner
                .usage
                .iter()
                .filter(|((aid, _), _)| *aid == account_id)
                .map(|(_, v)| *v)
                .sum()
        })
    }
}
