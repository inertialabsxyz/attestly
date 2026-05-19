//! Postgres-backed `Db` impl. Production target.
//!
//! Implementation note: uses the runtime-form `sqlx::query` / `query_as`
//! intentionally, not the compile-time-checked `query!` macros. The macros
//! require a live `DATABASE_URL` at `cargo check` time, which would break the
//! `make check` gate on contributors who don't have docker running. We pay
//! the small cost of dynamic statement preparation in return for a hermetic
//! build.
//!
//! Schema lives in `services/awp-cloud/migrations/` and is applied by
//! [`apply_migrations`] at boot.

use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use uuid::Uuid;

use crate::error::ApiError;
use crate::store::{
    Account, ApiKeyRecord, AttestationIndex, Db, IngestOutcome, SearchFilters, SearchPage,
    ShareLink,
};

pub struct PgDb {
    pool: PgPool,
}

impl PgDb {
    /// Connect to Postgres at `database_url`, capping the pool at
    /// `max_connections`. Migrations are applied separately via
    /// [`apply_migrations`] so the binary can fail loudly if the schema is
    /// out of date.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, ApiError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await
            .map_err(|e| ApiError::Internal(format!("postgres connect: {e}")))?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Apply every `.sql` file in `services/awp-cloud/migrations/` in lexical
    /// order. Idempotent: each file should be `CREATE ... IF NOT EXISTS` so
    /// re-running on a populated db is a no-op.
    ///
    /// We embed the migration text via `include_str!` rather than reading the
    /// filesystem at runtime, so a binary deploy carries its own schema.
    pub async fn apply_migrations(&self) -> Result<(), ApiError> {
        for (name, sql) in MIGRATIONS {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(|e| ApiError::Internal(format!("apply migration {name}: {e}")))?;
        }
        Ok(())
    }
}

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init.sql",
        include_str!("../../migrations/0001_init.sql"),
    ),
    (
        "0002_usage.sql",
        include_str!("../../migrations/0002_usage.sql"),
    ),
];

fn map_err(e: sqlx::Error) -> ApiError {
    ApiError::Internal(format!("postgres: {e}"))
}

fn row_to_account(row: sqlx::postgres::PgRow) -> Result<Account, ApiError> {
    Ok(Account {
        id: row.try_get("id").map_err(map_err)?,
        email: row.try_get("email").map_err(map_err)?,
        stripe_customer_id: row.try_get("stripe_customer_id").map_err(map_err)?,
        retention_days: row.try_get("retention_days").map_err(map_err)?,
        created_at: row.try_get("created_at").map_err(map_err)?,
    })
}

fn row_to_key(row: sqlx::postgres::PgRow) -> Result<ApiKeyRecord, ApiError> {
    Ok(ApiKeyRecord {
        id: row.try_get("id").map_err(map_err)?,
        account_id: row.try_get("account_id").map_err(map_err)?,
        project_name: row.try_get("project_name").map_err(map_err)?,
        hashed_key: row.try_get("hashed_key").map_err(map_err)?,
        created_at: row.try_get("created_at").map_err(map_err)?,
        revoked_at: row.try_get("revoked_at").map_err(map_err)?,
    })
}

fn row_to_index(row: sqlx::postgres::PgRow) -> Result<AttestationIndex, ApiError> {
    Ok(AttestationIndex {
        id: row.try_get("id").map_err(map_err)?,
        account_id: row.try_get("account_id").map_err(map_err)?,
        agent_id: row.try_get("agent_id").map_err(map_err)?,
        agent_pubkey_hex: row.try_get("agent_pubkey_hex").map_err(map_err)?,
        customer_id: row.try_get("customer_id").map_err(map_err)?,
        received_at: row.try_get("received_at").map_err(map_err)?,
        blob_sha256_hex: row.try_get("blob_sha256_hex").map_err(map_err)?,
    })
}

fn row_to_share_link(row: sqlx::postgres::PgRow) -> Result<ShareLink, ApiError> {
    let ids: Vec<Uuid> = row.try_get("attestation_ids").map_err(map_err)?;
    Ok(ShareLink {
        token: row.try_get("token").map_err(map_err)?,
        account_id: row.try_get("account_id").map_err(map_err)?,
        attestation_ids: ids,
        expires_at: row.try_get("expires_at").map_err(map_err)?,
        created_at: row.try_get("created_at").map_err(map_err)?,
        revoked_at: row.try_get("revoked_at").map_err(map_err)?,
    })
}

#[async_trait]
impl Db for PgDb {
    async fn ping(&self) -> Result<(), ApiError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn create_account(&self, email: &str) -> Result<Account, ApiError> {
        let row = sqlx::query(
            "INSERT INTO accounts (id, email, retention_days, created_at) \
             VALUES ($1, $2, 365, NOW()) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(email)
        .fetch_one(&self.pool)
        .await
        .map_err(map_err)?;
        row_to_account(row)
    }

    async fn create_api_key(
        &self,
        account_id: Uuid,
        project_name: &str,
        hashed_key: &str,
    ) -> Result<ApiKeyRecord, ApiError> {
        let row = sqlx::query(
            "INSERT INTO api_keys (id, account_id, project_name, hashed_key, created_at) \
             VALUES ($1, $2, $3, $4, NOW()) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(account_id)
        .bind(project_name)
        .bind(hashed_key)
        .fetch_one(&self.pool)
        .await
        .map_err(map_err)?;
        row_to_key(row)
    }

    async fn find_account_by_hashed_key(
        &self,
        hashed_key: &str,
    ) -> Result<Option<Account>, ApiError> {
        let row = sqlx::query(
            "SELECT a.* FROM accounts a \
             JOIN api_keys k ON k.account_id = a.id \
             WHERE k.hashed_key = $1 AND k.revoked_at IS NULL LIMIT 1",
        )
        .bind(hashed_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(row_to_account(r)?)),
            None => Ok(None),
        }
    }

    async fn list_active_hashed_keys(&self) -> Result<Vec<(String, Uuid)>, ApiError> {
        let rows =
            sqlx::query("SELECT hashed_key, account_id FROM api_keys WHERE revoked_at IS NULL")
                .fetch_all(&self.pool)
                .await
                .map_err(map_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let k: String = r.try_get("hashed_key").map_err(map_err)?;
            let a: Uuid = r.try_get("account_id").map_err(map_err)?;
            out.push((k, a));
        }
        Ok(out)
    }

    async fn insert_attestation(&self, index: AttestationIndex) -> Result<IngestOutcome, ApiError> {
        // ON CONFLICT DO NOTHING gives us the idempotency we want — combined
        // with a follow-up SELECT we can return Inserted vs Existed.
        let inserted = sqlx::query(
            "INSERT INTO attestations \
             (id, account_id, agent_id, agent_pubkey_hex, customer_id, received_at, blob_sha256_hex) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (id) DO NOTHING",
        )
        .bind(index.id)
        .bind(index.account_id)
        .bind(&index.agent_id)
        .bind(&index.agent_pubkey_hex)
        .bind(&index.customer_id)
        .bind(index.received_at)
        .bind(&index.blob_sha256_hex)
        .execute(&self.pool)
        .await
        .map_err(map_err)?
        .rows_affected();

        let row = sqlx::query("SELECT * FROM attestations WHERE id = $1")
            .bind(index.id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_err)?;
        let stored = row_to_index(row)?;
        Ok(if inserted == 1 {
            IngestOutcome::Inserted(stored)
        } else {
            IngestOutcome::Existed(stored)
        })
    }

    async fn fetch_attestation(
        &self,
        account_id: Uuid,
        id: Uuid,
    ) -> Result<Option<AttestationIndex>, ApiError> {
        let row = sqlx::query("SELECT * FROM attestations WHERE id = $1 AND account_id = $2")
            .bind(id)
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(row_to_index(r)?)),
            None => Ok(None),
        }
    }

    async fn search_attestations(
        &self,
        account_id: Uuid,
        filters: SearchFilters,
    ) -> Result<SearchPage, ApiError> {
        // Cursor format: "<received_at_rfc3339>|<id>" — lexicographically
        // ordered with received_at first means a stable descending scan even
        // under concurrent inserts.
        let cursor_at_id = match filters.cursor.as_deref() {
            None => None,
            Some(c) => {
                let mut parts = c.splitn(2, '|');
                let at = parts
                    .next()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
                let id = parts.next().and_then(|s| Uuid::from_str(s).ok());
                match (at, id) {
                    (Some(a), Some(i)) => Some((a.with_timezone(&Utc), i)),
                    _ => {
                        return Err(ApiError::InvalidRequest(
                            "malformed cursor — expected rfc3339|uuid".into(),
                        ))
                    }
                }
            }
        };

        let mut sql = String::from("SELECT * FROM attestations WHERE account_id = $1");
        let mut binds: Vec<String> = vec![];
        let mut idx = 2usize;
        if filters.agent_id.is_some() {
            sql.push_str(&format!(" AND agent_id = ${idx}"));
            idx += 1;
            binds.push("agent_id".into());
        }
        if filters.customer_id.is_some() {
            sql.push_str(&format!(" AND customer_id = ${idx}"));
            idx += 1;
            binds.push("customer_id".into());
        }
        if filters.from.is_some() {
            sql.push_str(&format!(" AND received_at >= ${idx}"));
            idx += 1;
            binds.push("from".into());
        }
        if filters.to.is_some() {
            sql.push_str(&format!(" AND received_at < ${idx}"));
            idx += 1;
            binds.push("to".into());
        }
        if cursor_at_id.is_some() {
            // Strictly less than (received_at, id) lexicographic descending.
            sql.push_str(&format!(
                " AND (received_at, id) < (${}, ${})",
                idx,
                idx + 1
            ));
            idx += 2;
            binds.push("cursor".into());
        }
        let _ = idx;
        sql.push_str(" ORDER BY received_at DESC, id DESC LIMIT ");
        // limit + 1 so we know if there's a next page
        sql.push_str(&(filters.limit + 1).to_string());

        let mut q = sqlx::query(&sql).bind(account_id);
        for b in &binds {
            q = match b.as_str() {
                "agent_id" => q.bind(filters.agent_id.as_deref().unwrap()),
                "customer_id" => q.bind(filters.customer_id.as_deref().unwrap()),
                "from" => q.bind(filters.from.unwrap()),
                "to" => q.bind(filters.to.unwrap()),
                "cursor" => {
                    let (a, i) = cursor_at_id.unwrap();
                    q.bind(a).bind(i)
                }
                _ => q,
            };
        }
        let rows = q.fetch_all(&self.pool).await.map_err(map_err)?;
        let mut indexes: Vec<AttestationIndex> = Vec::with_capacity(rows.len());
        for r in rows {
            indexes.push(row_to_index(r)?);
        }
        // We fetched `limit + 1` rows; if we got the overflow, drop it and
        // emit a cursor pointing at the last *kept* row. The next page's
        // strictly-less-than predicate then starts where this one stopped.
        let next_cursor = if indexes.len() > filters.limit {
            indexes.truncate(filters.limit);
            indexes
                .last()
                .map(|last| format!("{}|{}", last.received_at.to_rfc3339(), last.id))
        } else {
            None
        };
        Ok(SearchPage {
            rows: indexes,
            next_cursor,
        })
    }

    async fn list_all_attestations(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<AttestationIndex>, ApiError> {
        let rows = sqlx::query(
            "SELECT * FROM attestations WHERE account_id = $1 ORDER BY received_at ASC, id ASC",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(row_to_index(r)?);
        }
        Ok(out)
    }

    async fn create_share_link(&self, link: ShareLink) -> Result<(), ApiError> {
        sqlx::query(
            "INSERT INTO share_links \
             (token, account_id, attestation_ids, expires_at, created_at, revoked_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&link.token)
        .bind(link.account_id)
        .bind(&link.attestation_ids)
        .bind(link.expires_at)
        .bind(link.created_at)
        .bind(link.revoked_at)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn fetch_share_link(&self, token: &str) -> Result<Option<ShareLink>, ApiError> {
        let row = sqlx::query("SELECT * FROM share_links WHERE token = $1")
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(row_to_share_link(r)?)),
            None => Ok(None),
        }
    }

    async fn revoke_share_link(&self, account_id: Uuid, token: &str) -> Result<bool, ApiError> {
        let res = sqlx::query(
            "UPDATE share_links SET revoked_at = NOW() \
             WHERE token = $1 AND account_id = $2 AND revoked_at IS NULL",
        )
        .bind(token)
        .bind(account_id)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn record_usage(&self, account_id: Uuid, at: DateTime<Utc>) -> Result<(), ApiError> {
        let day: NaiveDate = at.date_naive();
        sqlx::query(
            "INSERT INTO usage (account_id, day, attestation_count) VALUES ($1, $2, 1) \
             ON CONFLICT (account_id, day) DO UPDATE SET attestation_count = usage.attestation_count + 1",
        )
        .bind(account_id)
        .bind(day)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn account_usage_total(&self, account_id: Uuid) -> Result<i64, ApiError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(attestation_count), 0)::BIGINT AS total \
             FROM usage WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_err)?;
        let total: i64 = row.try_get("total").map_err(map_err)?;
        Ok(total)
    }
}
