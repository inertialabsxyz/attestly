# Agent Prompts — Attestly Production Checklist

## Step 4 — Deploy, Stripe verification & backups (ops runbook)

**Branch:** `phase/4-deploy-and-billing-runbook`
**Depends on:** **Steps 1, 2, and 3 all merged to `main`.** This step deploys and
exercises the merged code against a live host, so it must reflect the final code:
the single-sourced base URL and admin-key fail-fast (Step 1), rate limiting and
tamper-evidence (Step 2), and the identity guarantee (Step 3).

**Prompt:**

This step is **ops and documentation**, not spec implementation. Your job: take
the deployment, Stripe-billing, hardening-alert, and backup items in
[`docs/PRODUCTION_CHECKLIST.md`](../docs/PRODUCTION_CHECKLIST.md) **§1**, **§2**,
and the two remaining **§4** items, and produce a **repeatable runbook** plus the
small config/doc changes those items require — then execute the parts that are
safe to execute (staging deploy, Stripe **test mode**) and record the results.
Read §1, §2, and §4 carefully, and skim "Explicitly out of scope."

You add **no production Rust** — all the code is merged. What you produce is:
(1) a checked-in runbook others can re-run, (2) any `fly.toml` / config / README
adjustments the checklist implies, and (3) captured evidence that the live flow
works. Where an action requires credentials or a live account you don't have,
**write the exact command and expected result into the runbook and mark it
`[BLOCKED — needs <credential>]`** rather than guessing or skipping silently.

### Context

Concrete current state (verified in the repo):

- **`services/attestly-cloud/fly.toml`** — `app = "attestly-cloud-staging"`,
  `primary_region = "iad"`, `force_https = true`, `min_machines_running = 1`.
  `[env]` sets `BIND_ADDR`, `BLOB_ROOT=/var/lib/attestly-cloud/blobs`,
  `RUST_LOG=info,attestly_cloud=info`,
  `ATTESTLY_CLOUD_BASE_URL=https://attestly-cloud-staging.fly.dev`. `[mounts]`
  binds volume `attestly_cloud_blobs` → `/var/lib/attestly-cloud/blobs` (matches
  `BLOB_ROOT`). `[http_service.checks.healthz]` GETs `/healthz` every 10s.
  `DATABASE_URL` and all `STRIPE_*` are **Fly secrets**, not in `fly.toml`.
- **Env the running binary needs** (from Step 1's single-sourced config):
  `DATABASE_URL` (required, no default — boot fails without it), `BLOB_ROOT`,
  `ATTESTLY_CLOUD_BASE_URL`, **`ATTESTLY_ADMIN_KEY`** (Step 1 makes boot
  **fail-fast** if this is empty or the `admin-key-placeholder` sentinel — so a
  real secret is now **mandatory** to start), and the Stripe set:
  `STRIPE_API_KEY`, `STRIPE_API_BASE` (default `https://api.stripe.com/v1`),
  `STRIPE_TEAM_PRICE_ID`, `STRIPE_OVERAGE_ITEM_ID`, `STRIPE_WEBHOOK_SECRET`.
  If Step 2 added `ATTESTLY_RATELIMIT_PER_MIN`, include it with its default.
- **Stripe is wired but unverified.** `src/stripe.rs` (`LiveStripeClient`,
  `webhook::verify` with a 5-min replay window + HMAC-SHA256 + constant-time
  compare) and `src/handlers/billing.rs` (`handle_stripe_webhook` →
  `checkout.session.completed` provisions an account + mints a one-time API key;
  `customer.subscription.*` syncs; `invoice.paid` logs). The webhook route is
  `POST /v1/billing/webhook`, signature-verified (not API-key gated).
- **Structured logging already exists** (`tracing` + `TraceLayer` +
  `EnvFilter`, `main.rs:33-71`); `/healthz` is the Fly health check. So the §4
  "structured logging" half is **done in code** — only the **alert** on
  `5xx`/healthcheck failure is outstanding, which is a Fly/monitoring config +
  doc task.
- **Blob backend is `FsBlobStore` on the Fly volume** — acceptable for a
  single-region pilot (S3 is explicitly deferred). The volume must survive a
  machine restart (checklist §1).
- **The tamper-evidence guarantee has an in-repo regression test from Step 2;**
  §4 additionally requires confirming it **on the live host** — a mutated at-rest
  blob → `HTTP 422 signature_invalid` and the red-banner share page.
- **Docs to update:** `services/attestly-cloud/README.md` (env/secrets, backups,
  restore) and `services/attestly-cloud/API.md` if any error slug changed in
  Step 2. There is **no Postgres test harness** in-repo; backup/restore is a
  managed-provider (Neon/Supabase) setting + a documented procedure.

### Your Task

Produce **`services/attestly-cloud/docs/PILOT_RUNBOOK.md`** (create the `docs/`
subdir under the service if absent) with the sections below, execute what is
safely executable, and make the small config/doc edits each item requires.
Commit logically (`cloud` scope for config/README/runbook, `docs` if you touch
top-level docs).

1. **Deploy the hosted service (§1).** Runbook section with the exact,
   copy-pasteable sequence and expected output for each:
   - `flyctl deploy` for `attestly-cloud-staging`.
   - Provision managed Postgres (Neon/Supabase per README) and
     `flyctl secrets set DATABASE_URL=...`.
   - Set `ATTESTLY_ADMIN_KEY` to a **strong** secret (`flyctl secrets set` —
     stress that Step 1 makes an unset/placeholder key **fail boot**, so this is
     mandatory, and note how to generate one).
   - Confirm the blob volume mounts at `BLOB_ROOT` and **survives a machine
     restart** — include the `flyctl machine restart` + re-fetch-a-blob check.
   - Confirm `ATTESTLY_CLOUD_BASE_URL` resolves to the real host and, thanks to
     Step 1, **share-link `url`s now use it** (no more `app.attestly.xyz`) — the
     smoke test below proves this.
   - Point real DNS + TLS at the app (custom domain wiring on top of Fly's
     `force_https`).
   - **Live smoke test:** `GET /healthz` → `200 {"status":"ok",...}`, then the
     full **ingest → search → share-link** flow **over HTTPS**, asserting the
     returned share `url` contains the real host (not the placeholder).

2. **Billing (Stripe), verified end to end in test mode (§2).** Runbook section:
   - Set `STRIPE_API_KEY`, `STRIPE_API_BASE`, `STRIPE_TEAM_PRICE_ID`,
     `STRIPE_OVERAGE_ITEM_ID`, `STRIPE_WEBHOOK_SECRET` as Fly secrets (test-mode
     keys/ids).
   - Register the webhook endpoint (`POST /v1/billing/webhook`) in the Stripe
     dashboard and confirm **signature verification passes** against
     `STRIPE_WEBHOOK_SECRET` (send a test event; expect a 2xx and a
     provisioning/sync log line).
   - Drive one real **Checkout → subscription-active → usage-recorded →
     overage-billed** cycle in Stripe **test mode**, step by step with the exact
     events and what each should produce (`checkout.session.completed` provisions
     an account + one-time API key; `customer.subscription.*` syncs; usage
     records; overage bills).
   - Confirm the **dashboard reflects plan + usage** after the webhook lands
     (`GET /dashboard`, and `GET /v1/account` + `GET /v1/account/usage` with the
     minted key).

3. **Alert on `5xx` / healthcheck failure (§4, logging half already in code).**
   - Note that structured logging is already in place (`TraceLayer` + `RUST_LOG`
     `EnvFilter`) and cite it — do not re-implement it.
   - Configure and document a **basic alert** on `5xx` responses and on
     `/healthz` check failure (Fly's healthcheck + a metrics/alert integration,
     or a simple external uptime monitor hitting `/healthz`). Write the exact
     setup steps and where the alert fires. If it needs an account you lack, mark
     `[BLOCKED — needs <account>]` with the precise steps ready to run.

4. **Automated Postgres backups + documented restore (§4).**
   - Enable automated backups on the managed provider (Neon/Supabase point-in-time
     or scheduled snapshots) and document the setting.
   - Write a **restore procedure** that a human can follow under pressure:
     provider restore → new `DATABASE_URL` → `flyctl secrets set` →
     redeploy/restart → `GET /healthz` green → spot-check a known attestation.
   - Add a short "Backups & restore" subsection to
     `services/attestly-cloud/README.md` linking the runbook.

5. **Tamper-evidence confirmed on the live host (§4).** Runbook section with the
   exact live steps: ingest an attestation, mutate its at-rest blob on the Fly
   volume (via the service's tamper path / an admin/ops step), then
   `GET /v1/share-links/:token` → **`HTTP 422 signature_invalid`** and
   `GET /share/:token` → the **red-banner** page. Cite the Step 2 in-repo test as
   the automated counterpart. If mutating a live blob isn't safe/available,
   document the exact procedure and mark `[BLOCKED — needs <access>]`.

6. **Config/doc edits the above imply.** If any secret is missing from `fly.toml`
   guidance, the README env table, or the runbook, add it. If Step 2 introduced
   `ATTESTLY_RATELIMIT_PER_MIN` or changed an error slug, reflect it in the
   runbook and `API.md`. Do **not** add production secrets to any committed file.

For every executable step you actually run, **paste the real observed output**
into the runbook (redacting secrets). For every step you cannot run, leave the
command + expected result and a `[BLOCKED — needs <credential>]` tag. The runbook
must be re-runnable end to end by the next operator.

### Do Not Touch

- Production Rust in `services/attestly-cloud/src/` — all code is merged (Steps
  1–2). This step edits `fly.toml`/README/`API.md`/runbook and runs ops
  commands; it does **not** change handlers, config-reading code, or the router.
- `python/attestly-langgraph/`, `crates/attestly-python/`, `crates/attestly-core/`
  — not part of deploy/billing/backup ops.
- **Real secrets in git** — every `STRIPE_*`, `DATABASE_URL`, `ATTESTLY_ADMIN_KEY`
  value goes to `flyctl secrets`, never into a committed file. Redact captured
  output.
- On-chain anchoring, S3/R2/B2 durable object storage, multi-region/SLA, SOC2,
  the Node SDK — **explicitly deferred** in the checklist. Document that the
  pilot runs single-region on the Fly volume with no SLA; do not implement or
  provision any of the deferred items.

### Closing the Loop

When the runbook is complete, executable steps are run (or `[BLOCKED]`-tagged
with reasons), and `make check` passes:

1. Spawn the review agent per [`.claude/review-gate.md`](../.claude/review-gate.md)
   against **`docs/PRODUCTION_CHECKLIST.md` §1, §2, and the §4 alert + backup +
   live-tamper items** — the review agent verifies every listed checklist box is
   either **executed with captured evidence** or **`[BLOCKED]` with a precise,
   ready-to-run procedure and the missing credential named** (no item silently
   skipped).
2. Capture the review agent's structured report.
3. Open a draft PR per [`.claude/pull-requests.md`](../.claude/pull-requests.md)
   (target `main`, title per [`.claude/commits.md`](../.claude/commits.md), e.g.
   `docs(cloud): add pilot deploy, stripe, and backup runbook`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline`
   with the review report, and list which checklist boxes are **done vs
   `[BLOCKED]`** so the remaining human ops work is explicit.

### Verification

```bash
make check
# → passes (config/doc edits don't break the gate; no code regressions)

# Runbook exists and is complete:
test -f services/attestly-cloud/docs/PILOT_RUNBOOK.md && echo present
# → present, with §1 deploy, §2 Stripe test-mode, §4 alert/backup/live-tamper sections

# Live smoke test (once deployed — from the runbook):
curl -s https://<real-host>/healthz
# → 200 {"status":"ok","version":"..."}

# Full flow over HTTPS returns a share url on the REAL host (Step 1 payoff):
#   ingest → search → create share-link → url starts with https://<real-host>/share/...
#   (NOT https://app.attestly.xyz) — captured in the runbook.

# Live tamper check (from the runbook):
#   GET /v1/share-links/<token> → HTTP 422 signature_invalid
#   GET /share/<token>          → red-banner tampered page
```
