# Agent Prompts — Attestly Production Checklist

## Step 1 — Deploy-Readiness: `attestly-cloud` config code fixes

**Branch:** `phase/1-cloud-deploy-readiness`
**Depends on:** _(none — this is the first, serial step)_

**Prompt:**

You are implementing the **code-side deploy-readiness fixes** for the Attestly
production checklist. The full specification is in
[`docs/PRODUCTION_CHECKLIST.md`](../docs/PRODUCTION_CHECKLIST.md) under **§1
Deploy the hosted service**. Read that section carefully before writing any
code, and skim the "Explicitly out of scope" section so you don't pull deferred
work forward.

Three checklist items in §1 are **code**, not ops (the ops half — `flyctl
deploy`, Postgres, DNS/TLS, live smoke test — is Step 4's job and you must not
attempt it). Your job is to make the code correct so that when Step 4 deploys,
share-link URLs resolve to the real host, a missing admin key fails fast instead
of silently running on a placeholder, and `/healthz` behaves as its contract
states.

You are also the **earliest cloud-touching agent**, so you own the shared
`attestly-cloud` config surface. Establish a **single source of truth for the
base URL** here; Step 2 (production hardening) will run in parallel afterwards
and must not need to touch these config files.

### Context

`services/attestly-cloud/` is its **own Cargo sub-workspace** (root `Makefile`
recurses into it via `$(MAKE) -C services/attestly-cloud`; `make check` at the
root runs `cloud-check`). Concrete current state:

- **`src/state.rs`** — `BillingConfig` struct with `from_env()` (`state.rs:17-56`)
  and `for_tests()`. It reads `ATTESTLY_CLOUD_BASE_URL` (default
  `https://app.attestly.xyz`, `state.rs:41`) into `base_url`, and
  `ATTESTLY_ADMIN_KEY` (default `admin-key-placeholder`, `state.rs:42`) into
  `admin_key`. **Both fall back to insecure placeholder strings if unset** —
  there is no fail-fast.
- **`src/handlers/share_links.rs`** — `public_url_for` (`share_links.rs:148-154`)
  **re-reads `ATTESTLY_CLOUD_BASE_URL` directly from the environment** and
  defaults to `https://app.attestly.xyz`, **bypassing `BillingConfig.base_url`**.
  This is the bug behind the checklist note "today it returns the placeholder
  `app.attestly.xyz`": there are two independent reads of the same var, and the
  share-link path uses the one that ignores config.
- **`src/handlers/health.rs`** — `GET /healthz` → `healthz()` (`health.rs:12`)
  pings **only** the DB (`state.db.ping()`) and returns 200
  `{"status":"ok","version":<CARGO_PKG_VERSION>}` or 503 `{"status":"degraded",...}`.
  The **doc comment (`health.rs:10`) claims it also checks blob storage**, which
  it does not — a doc/impl mismatch.
- **`src/state.rs` `AppState`** holds `db`/`blob`/`stripe`/`billing` as `Arc`s;
  `handlers` receive `State<AppState>`, so they can reach `state.billing.base_url`.
- Config is read inline in `main.rs` + `BillingConfig::from_env()`; there is no
  single unified `Config` struct and no `dotenv`. `fly.toml` sets
  `ATTESTLY_CLOUD_BASE_URL=https://attestly-cloud-staging.fly.dev`; Stripe vars
  and `DATABASE_URL` are Fly **secrets** (not in `fly.toml`).
- Tests are entirely in-memory (`tests/common/mod.rs` `Harness` over `MemDb` +
  `MemBlobStore`, driven via `tower::ServiceExt::oneshot`); `BillingConfig::for_tests()`
  supplies config. There is **no Postgres test harness** — do not add one.

### Your Task

Make three fixes, each its own logical commit with a test.

1. **Single-source the base URL (§1, "share-link `url`s resolve").** Make
   `public_url_for` in `src/handlers/share_links.rs` derive the share URL from
   **`state.billing.base_url`** (the value `BillingConfig::from_env()` already
   loads) instead of re-reading `ATTESTLY_CLOUD_BASE_URL` from the environment.
   After this change there must be **exactly one** read of `ATTESTLY_CLOUD_BASE_URL`
   in the service — in `BillingConfig::from_env()`. Grep to confirm:
   ```bash
   grep -rn "ATTESTLY_CLOUD_BASE_URL" services/attestly-cloud/src
   # → exactly one match, in src/state.rs
   ```
   - The handler already has `State<AppState>` (or can add it); reach the value
     via `state.billing.base_url`. Keep the returned format `{base}/share/{token}`.
   - **Test:** an integration test in `services/attestly-cloud/tests/share_links.rs`
     that constructs the `Harness` with a **non-default** `base_url` (extend
     `BillingConfig::for_tests()` or the harness so the test can inject one),
     creates a share link, and asserts the returned `url` starts with that
     injected base — proving the config value, not the `app.attestly.xyz`
     placeholder, is what surfaces. If `for_tests()` currently hard-codes a base,
     make it overridable without breaking existing callers (add a helper or an
     optional param; do not change the default any existing test relies on).

2. **Fail fast on a placeholder admin key (§1, "strong secret").** A production
   boot must not silently run with `ATTESTLY_ADMIN_KEY` unset. In the **binary
   startup path** (`src/main.rs`), after building `BillingConfig::from_env()`,
   **refuse to start** if `admin_key` is missing/empty or still equal to the
   `admin-key-placeholder` sentinel — log a clear error and exit non-zero. Do
   **not** move this check into `BillingConfig::from_env()` or `for_tests()`
   (tests and the in-memory harness rely on the placeholder). Keep the sentinel
   comparison in one named constant so the check and the default can't drift.
   - **Test:** a unit test asserting the guard predicate (e.g. a small
     `fn admin_key_is_insecure(&str) -> bool` in `state.rs` or `main`-adjacent
     module) returns `true` for `""` and for the placeholder, and `false` for a
     real secret. Keep the predicate pure so it's unit-testable without spawning
     a process.

3. **Reconcile `/healthz` with its contract (§1 smoke test target).** Resolve
   the `health.rs:10` doc/impl mismatch. Prefer **making the code match the
   documented contract**: have `healthz()` also confirm blob storage is
   reachable (a cheap liveness probe on `state.blob` — e.g. a `put`+`get` of a
   fixed tiny sentinel, or a dedicated cheap `health`/`ping` method you add to
   the `BlobStore` trait implemented by both `FsBlobStore` and `MemBlobStore`).
   On blob failure return the same **503 `{"status":"degraded",...,"detail":...}`**
   shape already used for DB failure. If you judge a blob probe too heavy for a
   healthcheck hit every 10s (see `fly.toml` interval), instead **correct the doc
   comment** to state DB-only — but pick one and make doc and impl agree.
   - **Test:** extend `services/attestly-cloud/tests/export_and_health.rs` to
     assert `GET /healthz` → 200 with `status: "ok"` on a healthy harness. If you
     added a blob probe + trait method, add a unit test for the new
     `MemBlobStore` probe path. Do **not** add a Postgres-backed test.

Keep every change inside `services/attestly-cloud/`. Use the `cloud` commit
scope. This step ships **no ops actions** — no `flyctl`, no secrets, no DNS.

### Do Not Touch

- `services/attestly-cloud/fly.toml` — Step 4's domain (ops/deploy config).
- `services/attestly-cloud/src/stripe.rs`, `src/handlers/billing.rs` — Stripe
  is Step 4 (verification is ops/config, not code); leave the webhook path alone.
- Any rate-limiting / auth-audit / tamper-test work — that is **Step 2's**
  domain. Do not add `governor`/`tower_governor`, do not add a rate-limit layer,
  do not add the tamper-evidence integration test. You own **config
  correctness**; Step 2 owns **hardening**.
- `python/attestly-langgraph/`, `crates/attestly-python/`, `crates/attestly-core/`
  — Step 3's tree and out of scope. Do **not** add a `health`/`ping` method to
  anything outside the cloud service's `BlobStore` trait.
- On-chain anchoring, S3 blob backend, LLM framework, Node SDK — **deferred by
  design** in the checklist. Do not pull forward.

### Closing the Loop

When implementation is complete and `make check` passes:

1. Spawn the review agent per [`.claude/review-gate.md`](../.claude/review-gate.md)
   against **`docs/PRODUCTION_CHECKLIST.md` §1 Deploy the hosted service** (the
   three code items: base-URL single-source, admin-key fail-fast, `/healthz`
   contract).
2. Capture the review agent's structured report.
3. Open a draft PR per [`.claude/pull-requests.md`](../.claude/pull-requests.md)
   (target `main`, title per [`.claude/commits.md`](../.claude/commits.md), e.g.
   `fix(cloud): single-source base url and fail fast on placeholder admin key`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline`
   with the review report.

### Verification

```bash
make check
# → passes (root gate recurses into services/attestly-cloud)

grep -rn "ATTESTLY_CLOUD_BASE_URL" services/attestly-cloud/src
# → exactly one match, in src/state.rs (share_links.rs no longer reads env)

# Admin-key fail-fast (run the binary without a real admin key):
cd services/attestly-cloud
ATTESTLY_ADMIN_KEY= DATABASE_URL=postgres://ignored cargo run --bin attestly-cloud 2>&1 | head -5
# → logs a clear "refusing to start: ATTESTLY_ADMIN_KEY ..." error and exits non-zero

# Healthz on the in-memory harness (asserted by the integration test):
cargo test --test export_and_health
# → healthz test passes with status "ok"
```
