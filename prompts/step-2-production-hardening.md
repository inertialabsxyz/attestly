# Agent Prompts — Attestly Production Checklist

## Step 2 — Phase 4: Minimum production hardening (`attestly-cloud`)

**Branch:** `phase/2-production-hardening`
**Depends on:** **Step 1 merged to `main`** (Step 1 owns the shared cloud config
surface — `state.rs`, `main.rs`, `lib.rs`, `share_links.rs`, `health.rs` — and
single-sources the base URL. Branch from `main` after Step 1 lands.)

**Runs in parallel with:** Step 3 (identity regression test). Step 3 is entirely
under `python/attestly-langgraph/` + `crates/attestly-python/`; you are entirely
under `services/attestly-cloud/`. You share no files — do not coordinate.

**Prompt:**

You are implementing **minimum production hardening** for `attestly-cloud`. The
full specification is in [`docs/PRODUCTION_CHECKLIST.md`](../docs/PRODUCTION_CHECKLIST.md)
under **§4 Minimum production hardening**. Read that section carefully before
writing any code, and skim "Explicitly out of scope" so you don't pull deferred
work forward.

Of §4's five items, three are **code you write here**: (a) an **auth audit
test** proving every `/v1/*` data route rejects unauthenticated requests, (b)
**rate limiting** on ingest + share-link creation, and (c) a **tamper-evidence
integration test** proving a mutated at-rest blob surfaces `HTTP 422
signature_invalid` and the share page renders the red banner. The other two §4
items — **structured logging + alert** and **automated Postgres backups +
restore doc** — are ops/config and belong to **Step 4**; do not do them here
(structured logging via `TraceLayer` + `EnvFilter` already exists in code —
leave it).

### Context

`services/attestly-cloud/` is its **own Cargo sub-workspace**; the root `make
check` recurses into it (`cloud-check`). Concrete current state:

- **Auth is an extractor, not middleware.** `AuthedAccount` (`src/auth.rs:95-114`)
  is an axum extractor validating `x-api-key` via an Argon2 scan over active key
  hashes. A route is protected **iff its handler takes an `AuthedAccount` param**.
  Protected today: `POST/GET /v1/attestations`, `GET /v1/attestations/:id`,
  `POST /v1/share-links`, `DELETE /v1/share-links/:token`, `GET /v1/export`,
  `POST /v1/billing/portal`, `GET /v1/account`, `GET /v1/account/usage`,
  `GET/POST/DELETE /v1/account/api-keys[...]`.
  **Deliberately public** (must stay public — assert they are, don't "fix" them):
  `GET /healthz`, `GET /`, `GET /dashboard`, `GET /quickstart`,
  `GET /v1/share-links/:token` + `GET /share/:token` (public redemption),
  `POST /v1/account/signup`, `POST /v1/telemetry/usage` (privacy contract),
  `POST /v1/billing/checkout` + `GET /billing/checkout` (pre-signup),
  `POST /v1/billing/webhook` (Stripe-signature-verified). Admin routes
  (`POST /v1/admin/bill-period`, `GET /v1/admin/telemetry/conversion-ready`) use
  a manual `x-admin-key` check, not `AuthedAccount`.
- Routes are wired in **`src/lib.rs` `router(state)` (`lib.rs:59-114`)**. The only
  layer applied is `TraceLayer::new_for_http()` (`main.rs:71`).
- **No rate limiting exists.** No `governor`/`tower_governor` dep. `tower-http`
  is pulled with `trace,cors,fs` features; note `cors` is enabled but
  `CorsLayer` is **never applied** — don't be misled into thinking a layer stack
  already exists beyond `TraceLayer`.
- **Tamper path already works server-side:** `src/canonical.rs` (`reverify`,
  `blob_sha256`, `parse_blob`) re-verifies blobs using `attestly-core`;
  `BlobStore::tamper_for_test` (`src/blob/mod.rs`) mutates an at-rest blob;
  `src/error.rs` maps a signature failure to the `signature_invalid` slug;
  `src/web.rs::tampered_html` renders the red-banner share page;
  `share_links.rs::render_page` (`share_links.rs:192`) returns it on 422. Your
  job is a **test that exercises this end to end**, not new tamper logic.
- **Tests are entirely in-memory.** `tests/common/mod.rs` `Harness::new()`
  builds `MemDb` + `MemBlobStore` + a Team account with a `default` API key +
  `MockStripeClient` + `BillingConfig::for_tests()`, and drives the router via
  `tower::ServiceExt::oneshot`. Integration files: `tests/{ingest,search,share_links,retrieval_and_tamper,billing,export_and_health,quickstart,telemetry}.rs`.
  There is **no Postgres test harness — do not add one.**

### Your Task

Three logical commits (each with tests), `cloud` scope.

1. **Auth audit test (§4, "every `/v1/*` route requires a valid API key").**
   Add `services/attestly-cloud/tests/auth_audit.rs`. For **every protected
   `/v1/*` route** listed above, assert that a request with **no `x-api-key`**
   (and one with a **wrong key**) is rejected — `401` (or the service's mapped
   unauthorized status/slug; check `src/error.rs` and `API.md` for the exact
   contract and assert against it, don't assume). For the **deliberately public**
   routes listed above, assert they are reachable **without** a key, so the test
   doubles as a guard that no future change accidentally locks a public route or
   opens a private one. Drive requests through the `Harness`. Keep the list of
   `(method, path, expected)` as a table in the test so a new route is one line
   to cover. This is a **test-only commit** — it must not change handler
   signatures; if a route you expect to be protected is *not*, that's a **finding**:
   protect it (add `AuthedAccount`) in this commit and note it in the PR body.

2. **Rate limiting on ingest + share-link creation (§4).** Add a rate limit to
   **`POST /v1/attestations`** (ingest) and **`POST /v1/share-links`**
   (share-link creation) only — not to reads, not to the public redemption GET,
   not globally. Wire it in `src/lib.rs router()` as a `tower` layer scoped to
   those two routes.
   - Prefer `tower_governor` (`governor`) keyed by the authenticated account so
     one tenant can't exhaust another; if per-account keying is impractical in
     the layer, key by client IP and document the choice in a code comment. Add
     the dep to `services/attestly-cloud/Cargo.toml` (this is a **cloud-local**
     dep — it does not touch the root workspace lockfile).
   - On limit exceeded, return **`429`** with the service's uniform
     `{error, detail}` envelope (extend `src/error.rs` with a `rate_limited`
     variant/slug so the response matches the existing contract shape; update
     `API.md` if it documents error slugs).
   - Make the limit **configurable** and set a sane pilot default (e.g. a
     per-minute burst). Read it from a new env var (e.g.
     `ATTESTLY_RATELIMIT_PER_MIN`) in `BillingConfig::from_env()` **or** a small
     adjacent config read, with a default that the in-memory `Harness` won't trip
     under normal test traffic.
   - **Test:** an integration test in a new `tests/rate_limit.rs` that fires more
     than the configured limit at `POST /v1/attestations` (with a valid key) and
     asserts the surplus requests get `429` with the `rate_limited` slug, while a
     request under the limit succeeds. Set the limit low **for that test's
     harness** so it's fast and deterministic — do not rely on wall-clock sleeps;
     if the limiter needs time to refill, prefer a test seam over `sleep`. Make
     sure the **existing** integration tests still pass under the default limit
     (raise the default or give the harness a high/disabled limit so they don't
     flake).

3. **Tamper-evidence end-to-end integration test (§4).** Add or extend a test
   (`tests/retrieval_and_tamper.rs` already exists — extend it, or add
   `tests/tamper_evidence.rs`) that: ingests an attestation via `POST
   /v1/attestations`, creates a share link, **mutates the at-rest blob** via
   `BlobStore::tamper_for_test`, then (a) fetches the share JSON
   (`GET /v1/share-links/:token`) and asserts **`HTTP 422`** with the
   **`signature_invalid`** slug, and (b) fetches the share **page**
   (`GET /share/:token`) and asserts the response renders the **red-banner
   tampered HTML** (assert on a stable marker string emitted by
   `web.rs::tampered_html`). This proves the guarantee the checklist calls out
   end to end in-process. (Step 4 confirms the **same** guarantee against the
   **live host**; your test is the in-repo regression guard.)

Keep every change inside `services/attestly-cloud/`. Do not touch the Python
tree, the bindings, or the core crate.

### Do Not Touch

- Step 1's config decisions — `ATTESTLY_CLOUD_BASE_URL` is **already
  single-sourced** through `BillingConfig::base_url` and the admin-key fail-fast
  is in `main.rs`. Do not re-read the base URL from the env or re-add a second
  read; do not alter the admin-key guard.
- `python/attestly-langgraph/`, `crates/attestly-python/`, `crates/attestly-core/`
  — **Step 3's tree**, running in parallel. Zero edits outside
  `services/attestly-cloud/`.
- `services/attestly-cloud/fly.toml`, structured-logging setup, alerting, and
  Postgres backups — **Step 4** (ops). `TraceLayer`/`EnvFilter` already exist;
  leave logging init alone.
- The tamper **logic** in `src/canonical.rs`, `src/error.rs` mappings for
  `signature_invalid`, and `web.rs::tampered_html` — these already work; you are
  writing a **test** around them (except adding the new `rate_limited` slug).
- On-chain anchoring, S3 backend, LLM framework, Node SDK — **deferred by
  design.** Do not pull forward.

### Closing the Loop

When implementation is complete and `make check` passes:

1. Spawn the review agent per [`.claude/review-gate.md`](../.claude/review-gate.md)
   against **`docs/PRODUCTION_CHECKLIST.md` §4 Minimum production hardening**
   (auth audit, rate limiting, tamper-evidence — noting that logging/alert and
   backups are Step 4's ops items, not gaps here).
2. Capture the review agent's structured report.
3. Open a draft PR per [`.claude/pull-requests.md`](../.claude/pull-requests.md)
   (target `main`, title per [`.claude/commits.md`](../.claude/commits.md), e.g.
   `feat(cloud): rate-limit ingest and share-link creation`). If the auth audit
   surfaced an unprotected route you had to protect, call it out explicitly in
   the body.
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline`
   with the review report.

### Verification

```bash
make check
# → passes (root gate recurses into services/attestly-cloud; no clippy warnings)

cd services/attestly-cloud

cargo test --test auth_audit
# → every protected /v1/* route returns 401 (or the mapped unauthorized slug)
#   with no/invalid key; every public route reachable without a key

cargo test --test rate_limit
# → requests over the configured limit return 429 with the rate_limited slug;
#   a request under the limit succeeds — deterministic, no wall-clock sleeps

cargo test --test retrieval_and_tamper   # (or tamper_evidence)
# → tampered blob: GET share JSON → 422 signature_invalid;
#   GET /share/:token → red-banner tampered HTML
```
