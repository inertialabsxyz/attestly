# Agent Prompts — GTM Phase 2 (SDK Wedge & Paid Conversion)

These prompts dispatch Claude Code agents to implement the six steps defined in [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md). Each prompt is self-contained — agents work in isolated git worktrees and do not share state during execution.

GTM Phase 1 ([`gtm-phase-1-plan.md`](gtm-phase-1-plan.md)) is complete: audit viewer, KYC receipts demo, persistent agent identity, and SR 11-7 pre-mapping are merged to `main`. Phase 2 converts that OSS-credibility surface into a paid funnel by shipping the LangGraph SDK (the adoption wedge) and `awp-cloud` MVP (the paid surface). The framing for *why* this sequence — OSS-core / paid-hosted, LangGraph as the regulated-buyer concentration point — lives in [`../awp-market-research.md`](../awp-market-research.md) and the Context section of `gtm-phase-2-plan.md`.

The conventions referenced below live in [`/.claude/`](../.claude/):

- [`commits.md`](../.claude/commits.md) — `make check` before every commit; `type(scope): description`
- [`testing.md`](../.claude/testing.md) — `make check` is the gate; tests required for features and regressions
- [`review-gate.md`](../.claude/review-gate.md) — review agent before PR
- [`pull-requests.md`](../.claude/pull-requests.md) — draft PR + Agent Run Report comment
- [`agent-prompts.md`](../.claude/agent-prompts.md) — the template these prompts follow

## Sequencing Overview

```
Week 1-2 (parallel):  Step 1 — PyO3 Bindings           Step 2 — awp-cloud Scaffold
                              │                                │
                              └────────────┬───────────────────┘
                                      merge to main
                                           │
Week 3-4 (parallel):  Step 3 — LangGraph SDK v0.1      Step 4 — Pricing & Billing
                              │                                │
                              └────────────┬───────────────────┘
                                      merge to main
                                           │
Week 5-6 (single):    Step 5 — Quickstart, Docs, Telemetry
                                           │
Week 7-8 (founder):   Step 6 — Design Partner #1 + LangSmith Integration
                                           │
                                      phase exit
```

**Sequencing rules:**

- **Step 1 and Step 2 may run in parallel** — Step 1 is Rust + PyO3 in a new `crates/awp-python` crate; Step 2 is a new hosted service in `services/awp-cloud/`. They share no files. Use separate git worktrees.
- **Both Step 1 and Step 2 must be merged to `main`** before Step 3 starts. Step 3 (the LangGraph SDK) consumes the PyO3 bindings for signing; Step 4 (Stripe billing) attaches to the `awp-cloud` service.
- **Step 3 and Step 4 may run in parallel** — Step 3 is Python in `python/awp-langgraph/`; Step 4 is Rust + HTML in `services/awp-cloud/` and `tools/landing-page/`. The only shared surface is the `CloudSink` ↔ `POST /v1/attestations` contract, which is locked by Step 2's exit criteria; both agents code to that contract independently.
- **Step 5 depends on Steps 3 and 4 both merged** — quickstart and docs assume the SDK exists, the cloud accepts attestations, and Stripe checkout works.
- **Step 6 is founder-led, not agent-dispatched.** The only engineering deliverable inside Step 6 is the LangSmith metadata integration (~2 days); the rest is contract close and case-study work that does not fit the dispatch model.

## Scope notes for the human

A few items in `gtm-phase-2-plan.md` are **not** for the dispatched agent:

- **Design Partner #1 close.** Step 6's signed-contract deliverable is founder-led outbound and procurement. The dispatched agent for Step 6 ships only the LangSmith metadata integration.
- **Real-PyPI publish gate.** Step 1 publishes to TestPyPI only. The promotion to `pypi.org` is gated on Step 6's design-partner close — that's a human decision, not an agent action.
- **`awp-cloud` production cutover.** Step 2's staging URL is the agent's exit bar. Pointing the real `app.awp-cloud.xyz` DNS, configuring secrets, and rotating the production Stripe key are operator tasks.
- **Pricing decisions.** The $499 Team tier and the $50k–$150k Enterprise indicative range are fixed in the plan — don't renegotiate them in the prompt. If the agent flags a concrete reason to revisit (e.g. Stripe minimum charge violations), surface to you; don't self-correct.
- **Telemetry opt-in vs opt-out posture.** Step 5 ships opt-out telemetry per the plan; if community signal turns negative in the first 30 days, the *human* decides whether to flip to opt-in. Do not pre-bake an opt-in default into the SDK.

The verification commands in each prompt assume `make check` exists (extended to cover the new crates/services as they land). Python-only and TypeScript-only steps still run `make check` from the repo root so the Rust workspace stays green; they additionally invoke their own per-package test commands.

---

## Step 1 — GTM Phase 2: PyO3 Bindings

**Branch:** `gtm-phase-2/pyo3-bindings`

**Prompt:**

You are implementing Step 1 of GTM Phase 2 of AWP. The full specification is in `planning/gtm-phase-2-plan.md` under "Step 1 — PyO3 Bindings". Read that section carefully before writing any code.

### Context

The repository contains a complete Rust prototype: `awp-core` (attestations, signing, Merkle batching, SQLite, persistent agent identity) and `awp-agents` (Worker, Verifier, Dispatcher, KycWorker, KycVerifier, Batcher). GTM Phase 1 added the static audit viewer (`tools/audit-viewer/`) and the `kyc_receipts` example.

The canonical signing payload lives in `crates/awp-core/src/attestation.rs::Attestation::signing_payload`. The audit viewer's JS already replicates it byte-for-byte. Your job is to expose the same signing path to Python via PyO3 with the same byte-identical guarantee — so that Python and Rust callers produce signatures that verify across both languages.

This step is **Rust + Python only.** Step 2 (`awp-cloud` MVP scaffold) ships in parallel in `services/awp-cloud/` — no merge collisions.

### Your Task

1. **Create `crates/awp-python/`** as a new workspace member containing a PyO3-bound crate. Add to the workspace `Cargo.toml`. Use `pyo3` with `abi3-py39` so a single wheel covers Python 3.9+ where possible. Module name: `awp_core_py` (exposed to Python as `awp`).

2. **Expose the signing API surface:**
   ```python
   awp.sign_attestation(payload: dict, identity: AgentIdentity) -> Attestation
   awp.verify_attestation(attestation: Attestation, pubkey: bytes) -> bool
   awp.AgentIdentity.load_or_create(path: str, agent_id: str) -> AgentIdentity
   awp.AgentIdentity.generate(agent_id: str) -> AgentIdentity
   ```
   - `Attestation` is a Python class backed by a Rust struct. Fields are accessible as Python attributes (`a.agent_id`, `a.agent_pubkey`, `a.task_hash`, `a.output_hash`, `a.output`, `a.status`, `a.references`, `a.timestamp`, `a.signature`).
   - `payload: dict` is converted to canonical bytes inside Rust — do **not** pre-serialise in Python. The Python-to-Rust dict conversion must produce the same canonical JSON bytes as a Rust `serde_json::to_vec` of an equivalent `serde_json::Value`.

3. **Byte-identical signatures across Rust and Python.** This is the load-bearing exit criterion. Ship a cross-language self-test:
   - A small Python script in `crates/awp-python/tests/cross_language.py` that loads a fixed payload from `tests/fixtures/payload.json`, signs in Python, asserts the resulting signature verifies in Rust (via a tiny Rust verifier binary `crates/awp-python/tests/verify_bin.rs` or a `cargo test`-driven harness — your call), and vice versa.
   - The test asserts bit-equality of the *signing payload bytes* (not just signature verification) — the canonical encoding must match exactly. Drift here will silently break in production.
   - Use a known test vector: payload + identity → expected signature hex. Hardcode the expected signature so any encoding drift fails the test loudly.

4. **`maturin`-based build.** Add `pyproject.toml` with `maturin` as the build backend. Local dev: `maturin develop` produces an importable wheel.

5. **CI matrix.** Add a GitHub Actions workflow `.github/workflows/python-wheels.yml` using `PyO3/maturin-action` that produces wheels for:
   - macOS (x86_64, arm64)
   - Linux (x86_64, aarch64 manylinux 2_28)
   - Windows (x86_64)
   - Python 3.9 through 3.13 (abi3 should collapse to one wheel per platform; if your `abi3-py39` config works, document that one wheel covers all versions)
   The workflow runs on tag push (`v*`) for publish and on every PR for build-only.

6. **Publish to TestPyPI** under the distribution name `awp-core-py` (Python import name remains `awp`). The publish step is gated by a manual workflow_dispatch in CI — don't publish on every push. Real PyPI publish is deferred to Step 6's design-partner gate; document this in the README.

7. **`crates/awp-python/README.md`** documenting:
   - Install: `pip install --index-url https://test.pypi.org/simple/ awp-core-py`
   - Usage example mirroring the API surface above
   - The byte-identical-signature guarantee, with a worked example showing a Python-signed attestation verifying in Rust
   - The FFI boundary: which Python types map to which Rust types
   - Limitations: no async API in v0.1, no streaming, single-process only

8. **Tests:**
   - Cross-language signing self-test (Python ↔ Rust, both directions) — required for exit
   - Unit tests in the Rust crate covering each exposed function
   - Python-side tests in `crates/awp-python/tests/test_api.py` using `pytest` covering identity load/save round-trip and signing roundtrip within Python

### Stubs to pre-place

This step touches `Cargo.toml` (workspace members list). Step 2 also lands a new workspace member (`services/awp-cloud/` — but it is a separate `Cargo.toml` outside the existing workspace, since it ships independently). **Confirm with Step 2's branch before merge that the workspace `Cargo.toml` does not collide.** Recommended: keep `services/awp-cloud/` out of the root workspace; declare it as its own workspace. This eliminates the merge conflict.

### Do Not Touch

- `services/awp-cloud/` — Step 2's domain, does not exist yet
- `python/awp-langgraph/` — Step 3's domain, does not exist yet
- `tools/landing-page/` — Step 4's domain for pricing-page edits; Step 1 has no reason to touch it
- Existing `crates/awp-core/` and `crates/awp-agents/` public APIs — additive only; do not break Rust callers. The four original examples and the `kyc_receipts` example must keep building unchanged.
- `Attestation::signing_payload` in `crates/awp-core/src/attestation.rs` — your Python canonical encoding must match this, not the other way around. If they disagree, Rust is the source of truth.

### Closing the Loop

When implementation is complete and `make check` passes (extended to run the Python tests):
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 1 — PyO3 Bindings", with emphasis on the exit criteria (especially "Cross-language signing self-test passes in both directions").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(python): gtm phase 2 pyo3 bindings for awp-core`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → Rust fmt + clippy clean, all Rust tests pass, Python tests pass

# Local development build
cd crates/awp-python && maturin develop
# → wheel built and installed into the active venv

# Cross-language byte-identical test
python -m pytest crates/awp-python/tests/cross_language.py -v
# → "sign_python_verify_rust PASSED"
# → "sign_rust_verify_python PASSED"
# → "canonical_bytes_match PASSED"

# Python-side API smoke
python -c "
import awp
ident = awp.AgentIdentity.generate('agent-test')
att = awp.sign_attestation({'task': 'hello'}, ident)
assert awp.verify_attestation(att, ident.public_key)
print('ok')
"
# → "ok"

# Existing Rust examples unaffected
cargo run --example kyc_receipts
cargo run --example dispatcher_flow
# → both succeed unchanged

# CI matrix produces wheels on tag push (verify in PR via workflow_dispatch dry run)
# → 4 platform wheels artefacted
```

---

## Step 2 — GTM Phase 2: `awp-cloud` MVP Scaffold

**Branch:** `gtm-phase-2/awp-cloud-scaffold`

**Prompt:**

You are implementing Step 2 of GTM Phase 2 of AWP. The full specification is in `planning/gtm-phase-2-plan.md` under "Step 2 — `awp-cloud` MVP Scaffold". Read that section carefully before writing any code.

### Context

The repository contains a complete Rust prototype plus GTM Phase 1 deliverables (audit viewer, KYC demo, persistent agent identity, SR 11-7 mapping). All attestations today flow through a local file (`data/attestations.json`) and the static viewer.

Your job is to build the hosted-service surface that converts that local flow into a paid product. Procurement-conservative buyers (Persona A, compliance lead Sarah) must be able to use AWP entirely offline via the OSS path; Persona B (platform operator Marcus) needs the hosted retention, sharing, and search to justify a paid pilot. The hosted service is the **paid wedge** — its existence makes Step 4's Stripe billing possible.

This step is **service code only.** Step 1 (PyO3 bindings) ships in parallel in `crates/awp-python/` — no merge collisions, since `services/awp-cloud/` is a separate workspace.

### Your Task

1. **Create `services/awp-cloud/`** as a new directory with its own `Cargo.toml` (own workspace, not part of the root workspace — this avoids workspace-member collisions with Step 1 and lets the service ship on its own release cadence). Stack:
   - **API:** Rust + Axum
   - **Storage:** Postgres (via `sqlx`) for indexed attestation metadata; S3-compatible blob storage (Minio in local dev) for raw signed payloads
   - **Auth:** API key (UUID, hashed in DB), scoped per project
   - **Deployment target:** Single container on Fly.io or Railway, managed Postgres (Neon or Supabase), Backblaze B2 or Cloudflare R2 for blob storage

2. **HTTP API surface** (versioned under `/v1/`):
   - `POST /v1/attestations` — body is a signed `Attestation` JSON. Server validates the signature (re-runs Ed25519 verify against the embedded `agent_pubkey`), rejects with 422 if invalid. On accept, stores the canonical bytes content-addressed by SHA-256, writes metadata row to Postgres, returns `{"id": "...", "received_at": "..."}`.
   - `GET /v1/attestations?agent_id=&customer_id=&from=&to=&cursor=&limit=` — paginated search. `customer_id` parses out of the `output` JSON when present (KYC scenario). Cursor-based pagination, default `limit=50`, max 500.
   - `GET /v1/attestations/{id}` — fetch a single attestation. Public if a valid share-link token is presented; otherwise requires the owning API key.
   - `POST /v1/share-links` — body `{"attestation_ids": [...]}` or `{"filter": {...}}`. Returns a tokenised URL `https://app.awp-cloud.xyz/share/{token}`. Tokens are scoped, time-limited (default 30 days, configurable), and revocable.
   - `GET /v1/export` — streams all attestations for the authenticated account as JSONL. Used by Step 4's "free to leave" guarantee.

3. **Signature handling rules (load-bearing):**
   - **Server never sees private keys.** All attestations arrive pre-signed.
   - **Tamper-evident retention.** Stored attestation bytes are content-addressed by SHA-256. On retrieval, the server re-runs the canonical encoding and re-verifies the signature before returning. Verification failure surfaces as `HTTP 422 Unprocessable Entity` with `{"error": "signature_invalid", "detail": "..."}`. The viewer must surface this prominently.
   - Re-use the existing canonical-bytes / signing-payload logic from `crates/awp-core` — depend on it as a workspace path (`{ path = "../../crates/awp-core" }`) so there is exactly one canonical encoder in the repo.

4. **Web viewer** at `services/awp-cloud/web/` — server-rendered (or single-page bundled into the binary) replica of the static audit viewer (Phase 1 Step 1), with server-backed search, share-link generation, and an account dashboard. **Reuse the existing static viewer's in-browser JS verification logic** — copy or vendor it; the viewer must independently re-verify, even on a server-backed page, so a compromised server cannot silently serve unverifiable receipts. Placeholder domain in code: `https://app.awp-cloud.xyz`.

5. **Local dev:** `docker compose up` (from `services/awp-cloud/`) brings up Postgres, Minio, and the API server. A `make seed` target inserts 10k synthetic attestations for testing search and pagination.

6. **Account model (for Step 4 to attach billing to):**
   - `accounts` table: id, email, stripe_customer_id (nullable until Step 4), retention_days (default 365), created_at
   - `api_keys` table: id, account_id, project_name, hashed_key, created_at, revoked_at (nullable)
   - `attestations` table: id, account_id, agent_id, agent_pubkey, customer_id (extracted, nullable), received_at, blob_sha256
   Use migrations via `sqlx-cli`. Migrations live in `services/awp-cloud/migrations/`.

7. **Deployment:** Provide a `Dockerfile`, `fly.toml` (or Railway equivalent), and a `services/awp-cloud/README.md` documenting the deployment model, the key-handling story (server never holds private keys), the data model, and current limitations (single region US-East, no SLA, no HA).

### Stubs to pre-place

For Step 3 (LangGraph SDK) and Step 4 (pricing/billing), both of which depend on this service, pre-place:

1. **A locked HTTP contract document** at `services/awp-cloud/API.md` describing the `POST /v1/attestations` request and response shapes — Step 3's `CloudSink` codes against this contract.
2. **A Stripe webhook stub** at `services/awp-cloud/src/billing.rs` containing an empty handler `pub async fn handle_stripe_webhook(...)` that returns 501. Step 4 fills it in; this prevents merge conflicts on the routing layer.
3. **A `usage` table migration** that records `(account_id, day, attestation_count)` — populated by an attestation-insert trigger or by a periodic aggregation job. Step 4 reads this for metered billing. Implementing the *recording* in Step 2 (so attestation volume is captured from day one) but **not** the *billing* (which is Step 4).

### Do Not Touch

- `crates/awp-python/` — Step 1's domain
- `python/awp-langgraph/` — Step 3's domain, does not exist yet
- `tools/landing-page/pricing.html` — Step 4's domain
- Existing `crates/awp-core/` and `crates/awp-agents/` public APIs — depend on them but do not modify them
- The static audit viewer at `tools/audit-viewer/` — that's the OSS path; your hosted viewer is a separate codebase, even if it shares verification JS

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 2 — `awp-cloud` MVP Scaffold", with emphasis on the exit criteria (especially "Tampered attestation surfaces as HTTP 422" and "Staging deployment live at a real URL").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(cloud): gtm phase 2 awp-cloud MVP scaffold`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report, plus the staging URL.

### Verification

```bash
make check
# → Rust fmt + clippy clean for both workspaces

# Local stack up
cd services/awp-cloud && docker compose up -d
# → postgres, minio, awp-cloud-api all healthy within 30s

# Seed and search
make seed
curl -H "x-api-key: $TEST_KEY" 'http://localhost:8080/v1/attestations?limit=10'
# → 10 attestations in JSON response, valid cursor

# Round-trip a real attestation
cargo run --example kyc_receipts                          # produces data/attestations.json locally
curl -X POST -H "x-api-key: $TEST_KEY" \
     -H "content-type: application/json" \
     -d @data/attestations.json \
     http://localhost:8080/v1/attestations
# → 201 with attestation id

# Share-link from incognito
TOKEN=$(curl -X POST -H "x-api-key: $TEST_KEY" -d '{"attestation_ids":["..."]}' http://localhost:8080/v1/share-links | jq -r .token)
curl "http://localhost:8080/share/$TOKEN"
# → renders the attestation publicly, no auth required

# Tamper test
# Manually mutate a byte of a stored blob in Minio, then re-fetch
curl -H "x-api-key: $TEST_KEY" "http://localhost:8080/v1/attestations/$ID"
# → HTTP 422 with {"error":"signature_invalid"}

# Staging deployment
flyctl deploy --config services/awp-cloud/fly.toml
# → https://awp-cloud-staging.fly.dev returns 200 on GET /healthz
```

---

## Step 3 — GTM Phase 2: LangGraph SDK v0.1

**Branch:** `gtm-phase-2/langgraph-sdk`
**Depends on:** Steps 1 and 2 merged to `main`

**Prompt:**

You are implementing Step 3 of GTM Phase 2 of AWP. The full specification is in `planning/gtm-phase-2-plan.md` under "Step 3 — LangGraph SDK v0.1". Read that section carefully before writing any code.

### Context

Steps 1 and 2 are complete and merged:

- `crates/awp-python/` exposes the Rust signing path to Python as the `awp` module on TestPyPI (Step 1). Byte-identical signatures are guaranteed across Rust and Python.
- `services/awp-cloud/` runs locally via `docker compose up` and accepts attestations at `POST /v1/attestations` (Step 2). The HTTP contract is locked at `services/awp-cloud/API.md`.
- GTM Phase 1 deliverables remain in place: the static audit viewer at `tools/audit-viewer/`, `kyc_receipts` example, and persistent identity at `data/identities/<agent_id>.json`.

Your job is to ship the adoption wedge: a one-line `pip install awp-langgraph` that wraps any LangGraph `StateGraph` and emits a signed attestation per node execution. This SDK is the marketing budget — every team that installs it is top-of-funnel for Step 6's design-partner conversion.

This step is **Python only.** Step 4 (pricing/billing) ships in parallel in `services/awp-cloud/src/billing.rs` and `tools/landing-page/pricing.html` — no merge collisions.

### Your Task

1. **Create `python/awp-langgraph/`** as a new Python package. Build backend: `hatchling` or `setuptools` (your call — favour `hatchling` for simplicity). Package name: `awp-langgraph`; import path: `awp.langgraph`. Depend on `awp-core-py` (Step 1's TestPyPI package) for signing.

2. **Primary API:**
   ```python
   from awp.langgraph import attest

   graph = build_my_graph()                       # user's existing StateGraph
   graph = attest(graph, agent_id="my-agent-01")  # wraps with attestation hooks
   ```
   The wrap must be idempotent (calling `attest()` twice on the same graph does not double-emit) and behaviour-preserving (same node outputs, same routing, same error propagation).

3. **One-line integration mechanics.** Wrapping a graph attaches a LangGraph callback (`langgraph` exposes a callback/listener interface — use the documented one; if multiple exist, pick the one most stable across LangGraph 0.2.x). On every node completion, the callback:
   - Hashes the node's input state and output delta (SHA-256, canonical JSON encoding)
   - Constructs an attestation payload: `{node_name, input_hash, output_hash, agent_id, timestamp}`
   - Signs via `awp.sign_attestation(payload, identity)`
   - Writes the resulting attestation to the configured sink

4. **Sinks (pluggable `Sink` protocol):**
   ```python
   class Sink(Protocol):
       def emit(self, attestation: Attestation) -> None: ...
   ```
   Provide three implementations:
   - `FileSink(path: str)` — append JSONL to a local file. Default when `sink=` is not passed.
   - `CloudSink(api_key: str, endpoint: str = "https://api.awp-cloud.xyz")` — POSTs to `awp-cloud` per `services/awp-cloud/API.md`. Retry on 5xx with exponential backoff (3 attempts max). Surface 422 (signature invalid) as a loud Python exception — that indicates the SDK is broken, not the network.
   - `CallableSink(fn: Callable[[Attestation], None])` — escape hatch.

5. **Dual-agent mode (optional, off by default):**
   ```python
   graph = attest(graph, agent_id="worker-01", verifier_agent_id="verifier-01")
   ```
   - Runs each node twice with two identities (sequentially in v0.1; parallel is a v0.2 nice-to-have)
   - Emits a Worker attestation and a Verifier attestation; the Verifier attestation embeds the verdict (`attestation_valid`, `answer_correct`)
   - Surfaces disagreement as a structured log line (not an exception — the user's graph should still run to completion). Documented prominently.

6. **Identity handling:**
   - Default: read `AgentIdentity` from `./data/identities/<agent_id>.json` (the persistent store landed in GTM Phase 1 Step 3). If the file is missing, generate and save it on first use.
   - Override: `attest(graph, agent_id=..., identity=my_identity_obj)` — explicit identity passed in.
   - The SDK never holds private keys in memory beyond the lifetime of the wrapped graph object. Document this in the README.

7. **No LLM dependency.** The SDK does not call an LLM itself; it observes whatever the LangGraph node already produces. This preserves the Phase 1 guardrail (`memory: project_autoagents_phase1.md`).

8. **Example** at `python/awp-langgraph/examples/kyc_graph.py` — a LangGraph version of the KYC demo from GTM Phase 1 Step 2, demonstrating the one-line wrap and end-to-end attestation flow. Three scenarios (Approve, Flag, Tampered) per the Phase 1 demo. The tampered scenario uses dual-agent mode and surfaces the disagreement.

9. **Performance budget:** signing adds <10ms per node on a modern laptop. Add a microbenchmark in `python/awp-langgraph/tests/bench_signing.py` (using `pytest-benchmark` or hand-rolled `time.perf_counter`). Document the measured number in the README. Ed25519 is cheap, but measure — JSON canonicalisation can be the slow path.

10. **README** at `python/awp-langgraph/README.md`: install, quickstart (the 5-line snippet from Step 5's quickstart works here), sink configuration, identity management, dual-agent mode, troubleshooting (especially "my attestations aren't appearing in awp-cloud" — most likely `CloudSink` 422 or wrong API key).

11. **Tests:**
    - Unit tests for each sink (file write round-trip; cloud sink against a mocked HTTP server; callable invocation)
    - Integration test: build a 3-node graph, wrap it, run it, assert 3 attestations written to a `FileSink` and that each one verifies against the audit viewer's verification path (call into `awp-core-py` for verification)
    - Byte-identical test: an attestation produced by `awp-langgraph` and an attestation produced by the Rust `kyc_receipts` example with the same input must have identical signing-payload bytes
    - Wrap-idempotence test
    - Dual-agent disagreement test (mock the second run to produce a different output; assert the Verifier attestation has `answer_correct: false`)

### Stubs to pre-place

None — this is the last step on the SDK side. Step 4 lands the pricing page and Stripe wiring in parallel; the only shared surface is the `CloudSink` ↔ `POST /v1/attestations` contract, which is locked by `services/awp-cloud/API.md` from Step 2.

### Do Not Touch

- `crates/awp-python/` — Step 1's domain (depend on the published TestPyPI package, do not modify the source in this branch)
- `services/awp-cloud/` other than reading `API.md` — Step 2's domain
- `tools/landing-page/pricing.html` — Step 4's domain
- Existing Rust examples (`kyc_receipts`, `dispatcher_flow`, etc.) — must keep working unchanged
- `tools/audit-viewer/` — your attestations must verify here unchanged

### Closing the Loop

When implementation is complete and `make check` passes (extended to run the LangGraph SDK tests):
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 3 — LangGraph SDK v0.1", with emphasis on the exit criteria (especially "Attestations from the Python SDK verify against the existing static audit viewer — byte-identical to Rust-produced attestations").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(sdk): gtm phase 2 langgraph SDK v0.1`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → Rust + Python tests pass

# Install the SDK
pip install --index-url https://test.pypi.org/simple/ awp-langgraph
# → installs along with awp-core-py

# Run the example
python python/awp-langgraph/examples/kyc_graph.py
# → 3 scenarios printed (Approve, Flag, Tampered)
# → data/attestations.jsonl populated with attestations from each node execution
# → tampered scenario surfaces Verifier disagreement

# Verify in the static audit viewer
open tools/audit-viewer/index.html
# → drag in data/attestations.jsonl
# → "✓ signature verified in browser" for every receipt
# → byte-identical to Rust-produced attestations

# Cloud sink against local awp-cloud
docker compose -f services/awp-cloud/docker-compose.yml up -d
AWP_API_KEY=test python python/awp-langgraph/examples/kyc_graph.py --sink cloud
curl -H "x-api-key: test" 'http://localhost:8080/v1/attestations?limit=10'
# → attestations appear in awp-cloud search results

# Performance budget
pytest python/awp-langgraph/tests/bench_signing.py -v
# → mean signing overhead <10ms per node on a modern laptop
```

---

## Step 4 — GTM Phase 2: Pricing, Billing, and Public Pricing Page

**Branch:** `gtm-phase-2/pricing-billing`
**Depends on:** Steps 1 and 2 merged to `main`

**Prompt:**

You are implementing Step 4 of GTM Phase 2 of AWP. The full specification is in `planning/gtm-phase-2-plan.md` under "Step 4 — Pricing, Billing, and Public Pricing Page". Read that section carefully before writing any code.

### Context

Steps 1 and 2 are complete and merged:

- `services/awp-cloud/` is running at a staging URL with the API key model in place (Step 2). A `usage` table records daily attestation counts per account. A Stripe webhook stub exists at `services/awp-cloud/src/billing.rs` returning 501.
- The landing page at `tools/landing-page/` already exists and was retargeted to Phase 2 / LangGraph in the `feat: phase 2 langraph focused` commit.

Your job is to convert the hosted service into a paid product: public pricing page, Stripe Checkout for self-serve Team-tier signup, metered billing for overage, retention enforcement, and a documented "free to leave" guarantee. This is the surface a procurement team will see before paying you.

This step is **service + landing page only.** Step 3 (LangGraph SDK) ships in parallel in `python/awp-langgraph/` — no merge collisions; the only shared surface is the locked `CloudSink` ↔ `POST /v1/attestations` contract.

### Your Task

1. **Public pricing page.** Add a new top-level section to `tools/landing-page/index.html` (or a separate `pricing.html`, your call — favour a single page if the visual flow allows, otherwise dedicate `pricing.html` and link from the main page's nav). Three tiers:
   - **OSS / Free** — $0. Local-first, file-based, you own everything. CTA: GitHub link.
   - **Team** — $499 / month. 1M attestations / month, 1-year retention, hosted viewer, email support. CTA: "Start free trial" → Stripe Checkout.
   - **Enterprise** — *Talk to us.* Unlimited attestations, 7-year retention, SSO, SOC 2 report (in progress, Phase 3), compliance templates, dedicated SE, MSA + DPA. **Do not put the $50k–$150k ACV range on the public page** — that's reserved for sales conversations. CTA: "Contact sales" → email or Calendly link.

2. **"Free to leave" guarantee** prominently on the pricing page: explicit statement that cancelling the paid tier does not invalidate existing receipts; users can export everything as JSONL via `GET /v1/export`. This is a procurement-conservative buyer's most common objection — answer it on the page.

3. **Stripe integration in `awp-cloud`** (`services/awp-cloud/src/billing.rs`):
   - **Stripe Checkout** for Team tier self-serve. On success, the webhook creates an account, issues an API key, emails the user the key, and links the account to the Stripe customer.
   - **Metered billing.** Beyond the 1M-attestation floor: $0.10 per additional 1k attestations, posted as Stripe usage records at end of each billing period. The `usage` table populated in Step 2 is the source of truth.
   - **Customer portal** — accessible from the `awp-cloud` account dashboard. Plan changes, invoices, and cancellation flow through Stripe's hosted portal (don't build a custom billing UI).
   - **Webhook signature verification** — non-negotiable. Stripe sends webhooks; the server must verify the signing secret before acting on any of them.

4. **Account-level retention enforcement.** Background sweeper (a separate process or a periodic Tokio task — your call) deletes attestations older than the account's `retention_days` (default 365 for Team, 2555 for Enterprise). OSS users have no account and never reach the cloud.

5. **Export endpoint** `GET /v1/export` — streams all attestations for the authenticated account as JSONL. Uses HTTP chunked transfer encoding. Output must be re-importable into the static audit viewer (Phase 1 Step 1) and re-verifiable end-to-end. Add an integration test confirming this round-trip.

6. **Account dashboard.** Add to the `awp-cloud` web viewer: current plan, API key management (rotate, revoke), usage chart (attestations per day for the last 30 days), Stripe portal link, export button.

7. **Landing-page visual consistency.** The pricing section must match the existing landing page's typography, colour, and spacing. If the existing landing page has a design system or CSS variables, use them. Do **not** rewrite the landing page styling — additive only.

### Stubs to pre-place

None — Step 5 (quickstart and docs) will link to the pricing page and Stripe Checkout flow, but does not require any code stubs from this step beyond the deployed pricing URL.

### Do Not Touch

- `crates/awp-python/` — Step 1's domain
- `python/awp-langgraph/` — Step 3's domain
- `services/awp-cloud/src/` other than `billing.rs` and the dashboard — keep the diff scoped to billing concerns
- The landing page's existing copy and visual language — additive only; do not rewrite the hero or product sections
- `tools/audit-viewer/` — the OSS viewer is separate from the hosted dashboard

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 4 — Pricing, Billing, and Public Pricing Page", with emphasis on the exit criteria (especially "Stripe Checkout signs up a new Team account, issues an API key, and accepts a test card" and "`GET /v1/export` returns valid JSONL that the static audit viewer can re-verify").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(cloud): gtm phase 2 stripe billing and public pricing`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report, plus a screenshot of the pricing page.

### Verification

```bash
make check
# → passes

# Pricing page renders cleanly
open tools/landing-page/index.html  # (or pricing.html)
# → three tiers visible, matches landing page visual language
# → "free to leave" guarantee prominent

# Stripe Checkout end-to-end (against Stripe test mode)
docker compose -f services/awp-cloud/docker-compose.yml up -d
# Click "Start free trial" → Stripe test card 4242 4242 4242 4242
# → account created, API key issued via email, dashboard accessible

# Metered billing
# Seed a synthetic account with 1.05M attestations for the current period
make seed-overage
# Manually trigger end-of-period billing
curl -X POST http://localhost:8080/v1/admin/bill-period -H "x-admin-key: ..."
# → Stripe usage record created for 50 units of overage ($5 surcharge)

# Customer portal
# Click "Manage subscription" in the dashboard
# → Stripe customer portal opens with current plan and invoice history

# Export and re-verify round-trip
curl -H "x-api-key: $TEST_KEY" http://localhost:8080/v1/export > exported.jsonl
open tools/audit-viewer/index.html
# → drag in exported.jsonl → all receipts render and verify

# Retention sweeper
make seed-old-attestations  # inserts 100 attestations dated 400 days ago
docker compose exec awp-cloud /usr/local/bin/sweeper run-once
# → 100 rows deleted from the test account
```

---

## Step 5 — GTM Phase 2: Quickstart, Docs, and Conversion Telemetry

**Branch:** `gtm-phase-2/quickstart-docs`
**Depends on:** Steps 3 and 4 merged to `main`

**Prompt:**

You are implementing Step 5 of GTM Phase 2 of AWP. The full specification is in `planning/gtm-phase-2-plan.md` under "Step 5 — Quickstart, Docs, and Conversion Telemetry". Read that section carefully before writing any code.

### Context

All prior Phase 2 steps are merged:

- Step 1: `awp-core-py` on TestPyPI (Rust signing exposed to Python)
- Step 2: `services/awp-cloud/` live at a staging URL with API key and Stripe-stub plumbing
- Step 3: `awp-langgraph` on TestPyPI (the one-line LangGraph wrapper)
- Step 4: Pricing page live; Stripe Checkout flowing; export and retention working

What's missing is the conversion surface itself: a 60-second quickstart that takes a curious LangGraph user from landing-page click to first signed receipt, a documentation site that procurement can review, and the anonymous telemetry that lets the GTM team identify which OSS users are conversion-ready.

This step is **docs + quickstart + telemetry only.** No new SDK features, no new cloud features.

### Your Task

1. **Quickstart at `https://awp-cloud.xyz/quickstart`** — a 60-second flow:
   1. Sign up (email + password, or OAuth — your call; favour email+password for simplicity; OAuth is a v0.2 polish)
   2. Display `pip install awp-langgraph` and the API key
   3. Copy-paste a 5-line snippet wrapping a tiny LangGraph example
   4. First attestation appears in the dashboard within seconds (live update via polling or WebSocket)
   The quickstart page is part of `services/awp-cloud/web/` — server-rendered.

2. **Documentation site at `docs.awp-cloud.xyz`.** Use a static-site generator that does not lock you into a vendor — `mkdocs` with the `material` theme is the recommended default (alternative: `docusaurus` if you prefer JS tooling). Source lives at `docs/site/` in the repo. Sections:
   - **Quickstart** — the 60-second path, with copy-paste snippets
   - **Concepts** — what's an attestation, what's a verifier, what's a sink, why does this matter
   - **LangGraph integration** — full SDK reference (auto-generated from `awp-langgraph` docstrings where possible)
   - **Self-hosted** — OSS-only path (FileSink, static viewer, no cloud)
   - **Compliance** — pointers to `docs/compliance/SR_11_7.md` from GTM Phase 1 Step 4
   - **Migration** — moving from `FileSink` to `CloudSink` without losing attestations (the export endpoint from Step 4 is the answer)
   Each section has at least one runnable example.

3. **Anonymous SDK telemetry** in `awp-langgraph` (opt-out by default per the plan; if the human flags community pushback in the first 30 days, the decision to flip to opt-in is theirs, not yours):
   - Emits a daily aggregate POST to `https://telemetry.awp-cloud.xyz/v1/usage`:
     ```json
     {"install_id": "<uuid generated on first import>",
      "sdk_version": "0.1.0",
      "attestations_emitted_today": 4231,
      "sink_type": "FileSink",
      "python_version": "3.11.5",
      "os": "darwin"}
     ```
   - **No payload data. No agent IDs. No customer IDs. No user identifiers beyond the install UUID.** The install UUID is generated locally on first import and stored at `~/.config/awp/install_id`.
   - Opt-out via `AWP_TELEMETRY=0` environment variable or `awp.langgraph.telemetry_disable()`. Documented prominently in the SDK README's first 200 words.
   - Server endpoint in `services/awp-cloud/` aggregates daily counts in a separate `telemetry_events` table (no PII, no joins to `accounts`).
   - Used internally to identify conversion-ready OSS users (>10k attestations/day on FileSink is the threshold called out in the plan); surfaced in an admin dashboard route gated by admin key.

4. **Landing page integration.** Update `tools/landing-page/index.html` to add a "Try it on your LangGraph agent" CTA that links to the quickstart. Place it near the existing hero — keep the change small and visually consistent.

5. **Root `README.md` update.** Lead the install section with the Python SDK (`pip install awp-langgraph`), and demote the Rust prototype examples to a "Reference implementation" subsection. Keep the existing market research and prototype-plan links intact.

### Stubs to pre-place

None — this step has no downstream dependencies inside Phase 2. Step 6's LangSmith integration is a small, self-contained engineering task that does not need stubs from here.

### Do Not Touch

- `crates/awp-python/` — Step 1's domain
- `services/awp-cloud/src/` other than the quickstart route and the telemetry ingest endpoint — keep the diff scoped to docs/quickstart/telemetry
- `python/awp-langgraph/` other than adding the telemetry module — additive only, no API changes
- `tools/landing-page/index.html` beyond the single CTA addition — preserve the existing copy and design
- `tools/audit-viewer/` — Phase 1's surface; reference it in the self-hosted docs but do not modify it

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 5 — Quickstart, Docs, and Conversion Telemetry", with emphasis on the exit criteria (especially "Quickstart can be completed end-to-end by a new user in under 5 minutes" and "Opt-out works: `AWP_TELEMETRY=0` produces no network calls").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `docs: gtm phase 2 quickstart, docs site, and conversion telemetry`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report, plus the docs-site staging URL.

### Verification

```bash
make check
# → passes

# Quickstart end-to-end (manual, timed)
# Open the staging URL → sign up → install SDK → paste snippet → see first attestation
# → completes in under 5 minutes for a fresh user

# Docs site builds and serves
cd docs/site && mkdocs serve
# → six sections present, each with at least one runnable example

# Telemetry opt-in default works
python python/awp-langgraph/examples/kyc_graph.py
# (use mitmproxy or wireshark)
# → one POST to telemetry.awp-cloud.xyz/v1/usage with aggregate stats

# Telemetry opt-out works
AWP_TELEMETRY=0 python python/awp-langgraph/examples/kyc_graph.py
# → zero network calls to telemetry domain (verified via mitmproxy)

# Install ID is stable across runs but anonymous
cat ~/.config/awp/install_id
# → uuid, not tied to any user-identifying information

# Landing page CTA
open tools/landing-page/index.html
# → "Try it on your LangGraph agent" CTA visible and links to /quickstart

# README leads with Python SDK
head -20 README.md
# → `pip install awp-langgraph` appears in the install section
# → Rust prototype demoted to "Reference implementation" subsection
```

---

## Step 6 — GTM Phase 2: LangSmith Metadata Integration

**Branch:** `gtm-phase-2/langsmith-integration`
**Depends on:** Step 3 merged to `main`

**Prompt:**

You are implementing the engineering portion of Step 6 of GTM Phase 2. The full Step 6 specification is in `planning/gtm-phase-2-plan.md` under "Step 6 — Design Partner #1 + LangSmith Integration". **Most of Step 6 is founder-led (contract close, case study, outbound) — your scope is the LangSmith metadata integration only (~2 days of engineering).** Read the spec section before writing code, but only the LangSmith integration is yours.

### Context

GTM Phase 2 Steps 1–5 are merged. The `awp-langgraph` SDK ships with `FileSink`, `CloudSink`, and `CallableSink`. The next step in the GTM funnel is signing Design Partner #1 — a regulated buyer already running LangGraph in production. Most such teams also use LangSmith for tracing and observability. AWP must show up as **additive to LangSmith**, not as a competitive replacement, or the conversation never reaches the pilot stage.

Your job is to add an option that injects AWP attestation IDs into LangSmith trace metadata, so a user looking at a LangSmith trace can click through to the corresponding AWP attestation in `awp-cloud`. This is small but load-bearing: it's the single most-effective procurement-conservative-buyer reassurance ("we're not asking you to rip out anything").

### Your Task

1. **Add a `langsmith_callback` option** to `attest()` in `python/awp-langgraph/`:
   ```python
   graph = attest(graph, agent_id="my-agent-01", langsmith_callback=True)
   ```
   When enabled, the SDK detects whether the active LangGraph run has a LangSmith tracer attached (via `langsmith.run_helpers.get_current_run_tree()` or the equivalent stable API for the LangSmith version pinned by `awp-langgraph`).

2. **Inject attestation metadata into LangSmith traces.** For each node execution that produces an attestation, write the attestation ID into the corresponding LangSmith run's metadata:
   ```python
   run.add_metadata({"awp_attestation_id": attestation.id,
                     "awp_attestation_url": f"{cloud_endpoint}/attestations/{attestation.id}"})
   ```
   The metadata write must not fail the user's graph if LangSmith is unavailable — degrade silently to a warning log line, do not raise.

3. **Reverse link in the `awp-cloud` viewer.** Update `services/awp-cloud/web/` (or the static viewer if both are shared) so that when an attestation has a LangSmith trace ID in its metadata, the receipt detail view shows a "View in LangSmith" link pointing to `https://smith.langchain.com/o/<org>/r/<trace_id>` (URL pattern documented in LangSmith's public docs). This is the "additive, not competitive" UX moment.

4. **Test against a real LangSmith workspace.** Create a test LangSmith account (free tier is fine) and run the `kyc_graph.py` example with `langsmith_callback=True`. Confirm:
   - Attestation IDs appear in the LangSmith trace metadata
   - The "View in LangSmith" link in the AWP viewer opens the correct trace
   - Disabling the option produces no LangSmith calls
   Document the test workspace configuration in `python/awp-langgraph/tests/test_langsmith_integration.py` (with credentials read from env vars; don't commit any).

5. **Documentation.** Add a "LangSmith integration" section to `docs.awp-cloud.xyz/concepts/` and to `python/awp-langgraph/README.md`. Frame as: "AWP + LangSmith — provenance and observability in one trace." (The eventual joint blog post with LangChain reuses this framing; coordinate with the founder before publishing externally.)

### Stubs to pre-place

None — this is the last engineering step in Phase 2.

### Do Not Touch

- `crates/awp-python/` — not your domain
- `services/awp-cloud/src/` other than the viewer template update for the LangSmith link — keep the diff scoped
- `python/awp-langgraph/` core API surface — additive only, do not change existing signatures
- Anything related to the contract, pricing, or case study — those are founder-led; do not draft contract language or pricing terms in this branch

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-2-plan.md` → "Step 6 — Design Partner #1 + LangSmith Integration", with emphasis on the LangSmith integration exit criteria only (the design-partner-close items are out of scope for this branch).
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(sdk): gtm phase 2 langsmith metadata integration`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report, plus a screenshot showing the AWP attestation ID inside a real LangSmith trace.

### Verification

```bash
make check
# → passes

# LangSmith integration end-to-end (requires LANGSMITH_API_KEY in env)
LANGSMITH_API_KEY=ls_... python python/awp-langgraph/examples/kyc_graph.py --langsmith
# → attestations emitted as usual
# → LangSmith traces include awp_attestation_id metadata
# → check https://smith.langchain.com — metadata visible in trace detail view

# Reverse link in viewer
# Open https://app.awp-cloud-staging.fly.dev (or local docker stack)
# → receipt detail shows "View in LangSmith" link
# → clicking opens the correct trace

# Silent degradation when LangSmith unavailable
unset LANGSMITH_API_KEY
python python/awp-langgraph/examples/kyc_graph.py --langsmith
# → graph runs to completion; warning log line emitted; no exception
# → AWP attestations still emitted normally
```
