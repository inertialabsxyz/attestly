# `attestly-cloud`

Hosted ingest, search, and share-link service for Attestly attestations. The paid
surface that makes [`planning/gtm-phase-2-plan.md`](../../planning/gtm-phase-2-plan.md)'s
Stripe billing possible (Step 4). Procurement-conservative buyers keep the
OSS path; Persona B (platform operator Marcus) gets retention, sharing, and
search to justify a paid pilot.

## What it is, what it isn't

- **Is:** the hosted ingest of signed attestations, indexed in Postgres,
  stored content-addressed in a blob layer, retrievable individually or via
  paginated search, shareable via tokenised time-limited URLs, exportable as
  JSONL.
- **Isn't:** a multi-region service. Single region (`iad`), single AZ, no
  SLA. This is explicit on the public pricing page; multi-region is a Phase 3
  enterprise concern.
- **Isn't:** a key custodian. The server **never** sees a private key. Every
  attestation arrives pre-signed; we validate and re-validate the embedded
  signature, but no signing material crosses the network boundary.

## Locked HTTP contract

The full surface lives in [`API.md`](./API.md). The LangGraph SDK (`CloudSink`
in GTM Phase 2 Step 3) and Stripe billing (Step 4) both code against it.
After this document is merged to `main`, the request and response shapes of
`/v1/*` will not change without a new major version namespace.

The load-bearing contracts:

1. **Server never holds private keys.** `POST /v1/attestations` accepts a
   pre-signed `Attestation` and verifies it server-side against the embedded
   `agent_pubkey`. The signature path re-uses
   [`attestly-core::Attestation::signing_payload`](../../crates/attestly-core/src/attestation.rs)
   via a workspace path dependency, so there is exactly one canonical encoder
   in the repo.
2. **Tamper-evident retention.** Every retrieval (`GET /v1/attestations/{id}`,
   share-link redemption, `GET /v1/export`) re-runs the canonical encoding,
   recomputes the content hash, and re-verifies the signature. If the at-rest
   bytes have drifted in any way — operator error, corruption, deliberate
   tamper — the route surfaces `HTTP 422 signature_invalid` and the hosted
   viewer renders a red banner instead of the receipts. Auditors can rely on
   the 200 response as a fresh cryptographic statement, not a stale
   acceptance record.
3. **Viewer re-verifies in-browser.** `/share/{token}` ships the same
   vendored Ed25519 verifier as the OSS audit viewer
   (`tools/audit-viewer/vendor/ed25519.js`) and the same canonical encoder
   (`tools/audit-viewer/signing-payload.js`). The server's bytes get checked
   in the auditor's browser; a compromised server cannot silently serve
   unverifiable receipts.

## Architecture

```
   ┌──────────┐   POST /v1/attestations    ┌────────────┐
   │ LangGraph│ ────signed Attestation───▶ │            │
   │   SDK    │                            │            │
   └──────────┘                            │            │
                                           │ Axum API   │
   ┌──────────┐   GET /share/{token}       │ (Rust)     │
   │ Auditor  │ ─────────public──────────▶ │            │
   │ browser  │ ◀──server-rendered HTML─── │            │
   └──────────┘ (re-verifies in-browser)   └─────┬──────┘
                                                 │
                                  ┌──────────────┼──────────────┐
                                  │              │              │
                            ┌─────▼─────┐  ┌─────▼──────┐  ┌────▼─────┐
                            │ Postgres  │  │ Blob store │  │  Stripe  │
                            │ (sqlx)    │  │ (FS / S3)  │  │ (Step 4) │
                            └───────────┘  └────────────┘  └──────────┘
```

- **API:** Rust + Axum 0.7
- **Indexed metadata:** Postgres 16 via `sqlx` 0.8 (dynamic queries; no
  compile-time DB requirement so `cargo check` is hermetic)
- **Blob storage:** content-addressed by SHA-256. v0.1 ships a filesystem
  impl ([`blob::filesystem::FsBlobStore`](./src/blob/filesystem.rs)) used
  locally and on the first Fly volume deploy. The trait
  ([`blob::BlobStore`](./src/blob/mod.rs)) is the seam — production swap to
  Backblaze B2 / Cloudflare R2 is a one-line change in `main.rs`.
- **Auth:** API keys (UUIDs) hashed with Argon2id at rest. `x-api-key`
  header.
- **Deployment:** single container on Fly.io (`fly.toml` checked in),
  managed Postgres (Neon or Supabase), Fly volume for blob storage during
  v0.1.

## Data model

```
accounts(id, email, stripe_customer_id, retention_days, created_at)
api_keys(id, account_id, project_name, hashed_key, created_at, revoked_at)
attestations(id, account_id, agent_id, agent_pubkey_hex,
             customer_id, received_at, blob_sha256_hex)
share_links(token, account_id, attestation_ids[], expires_at,
            created_at, revoked_at)
usage(account_id, day, attestation_count)
```

Migrations in [`migrations/`](./migrations/) are applied automatically at
service boot.

The `usage` table is populated on every successful ingest. Stripe metered
billing (Step 4) reads it without further plumbing — Step 2 makes sure
nothing is missed from day one.

## Local dev

Requires Docker.

```bash
# Bring up Postgres + Minio + the API server
docker compose up -d --build

# Wait a few seconds for healthchecks, then seed 10k attestations
make seed           # prints `export TEST_KEY=<uuid>` — eval it in your shell

# Hand-test
curl http://localhost:8080/healthz
curl -H "x-api-key: $TEST_KEY" 'http://localhost:8080/v1/attestations?limit=10' | jq

# Round-trip a real attestation produced by the OSS examples
cargo run --example kyc_receipts        # from the repo root
curl -X POST -H "x-api-key: $TEST_KEY" \
     -H "content-type: application/json" \
     -d @data/attestations.json \
     http://localhost:8080/v1/attestations
```

Tear it down:

```bash
make down        # stop containers, keep volumes
make clean       # stop + wipe state
```

## Tests

```bash
make check        # cargo fmt + clippy + cargo test
```

The test suite runs against the in-memory `Db` and `BlobStore`
implementations, so `make check` has no external dependencies (no Docker, no
Postgres) and runs in seconds. The Postgres impl is exercised by the seed
binary and by the staging deploy; behaviour is pinned by the trait
contract.

## Verifying the tamper guarantee by hand

```bash
# Ingest one attestation
ID=$(curl -s -X POST -H "x-api-key: $TEST_KEY" \
        -H "content-type: application/json" \
        -d @data/attestations.json \
        http://localhost:8080/v1/attestations | jq -r .id)

# Round-trip is clean
curl -s -H "x-api-key: $TEST_KEY" "http://localhost:8080/v1/attestations/$ID" | jq .

# Locate the blob (sha256 returned by ingest) and corrupt it
SHA=$(...)
docker compose exec attestly-cloud /bin/sh -c "echo garbage > /var/lib/attestly-cloud/blobs/${SHA:0:2}/${SHA:2:2}/$SHA"

# Re-fetch — must 422
curl -s -H "x-api-key: $TEST_KEY" "http://localhost:8080/v1/attestations/$ID"
# → {"error":"signature_invalid","detail":"...possible tamper at rest"}
```

## Staging deploy

```bash
flyctl launch --no-deploy --copy-config --name attestly-cloud-staging
flyctl secrets set DATABASE_URL=postgresql://...   # Neon connection string
flyctl secrets set ATTESTLY_CLOUD_BASE_URL=https://attestly-cloud-staging.fly.dev
flyctl deploy --config fly.toml
flyctl status      # https://attestly-cloud-staging.fly.dev/healthz must return 200
```

The DNS for the production `app.attestly.xyz` cutover is **not** an agent
action; it's tracked in Step 6's founder-led tasks.

## Limitations and deliberate omissions

These are explicit in the plan and called out here so reviewers don't flag
them as gaps:

- **Single region, single AZ.** No HA. No SLA on the public pricing page.
  Multi-region is a Phase 3 enterprise concern.
- **No SOC 2 yet.** Pursued in GTM Phase 3 once a paying pilot exists; Type
  I report is a Month 4–6 deliverable, not Month 3.
- **No Stripe billing yet.** Webhook route exists (returns 501) so Step 4
  can fill in without changing the routing layer. The `usage` table is
  populated from day one so retroactive metered billing is possible.
- **No S3 blob backend yet.** The filesystem impl is the v0.1 production
  blob store, deployed to a Fly volume. The trait is the seam; S3 lands when
  the staging deploy moves off Fly volumes.
- **API-key auth is per-row Argon2 scan.** Fine for v0.1's key population
  (single digits per account). A fast lookup column is a Phase-3 perf
  upgrade.
- **No retention sweeper yet.** `accounts.retention_days` is recorded but
  not enforced; the sweeper ships with Step 4 alongside Stripe (the
  "cancelling does not delete your receipts" guarantee depends on it).
