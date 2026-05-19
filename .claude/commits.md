# Commits

**Gate:** `make check` must pass before every commit. No exceptions.

**Auto-commit:** Commit each logical change as it is completed, without waiting to be asked. Use judgment to determine when a change is coherent and complete — do not commit mid-feature or bundle unrelated changes.

**Message format:** `type(scope): short description`

- `type` — `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
- `scope` — the crate, service, tool, or doc surface being changed. Use the smallest accurate scope from the list below; if a change genuinely spans multiple, pick the dominant one and mention the others in the body.
- Description — imperative, lowercase, no period. 72 characters total max.

## Scopes in use

Derived from the repo layout and existing history — match these exactly when possible:

- **`core`** — `crates/awp-core/` (attestations, signing, identity, merkle, storage, kyc types)
- **`agents`** — `crates/awp-agents/` (Worker, Verifier, Dispatcher, Batcher, KYC agents, tools)
- **`examples`** — `crates/awp-examples/` (`simple_attestation`, `dispatcher_flow`, `kyc_receipts`, etc.)
- **`viewer`** — `tools/audit-viewer/` (the static HTML+JS audit viewer)
- **`landing`** — `tools/landing-page/` (the marketing/pricing site)
- **`compliance`** — `docs/compliance/` (SR 11-7 mapping and future regulation docs)
- **`docs`** — other docs (`docs/ARCHITECTURE.md`, `docs/USER_JOURNEYS.md`, root `README.md`)
- **`plan`** — anything under `planning/` (prototype plan, GTM plans, agent-prompts files)
- **`python`** — `crates/awp-python/` (PyO3 bindings; lands in GTM Phase 2)
- **`sdk`** — `python/awp-langgraph/` (LangGraph SDK; lands in GTM Phase 2)
- **`cloud`** — `services/awp-cloud/` (hosted service; lands in GTM Phase 2)
- **`ci`** — `.github/workflows/`, Makefile, repo-wide tooling
- **`deps`** — dependency-only changes (`Cargo.toml`, lockfiles)

If a new top-level surface lands and none of the above fit, add the scope here in the same commit that introduces the surface.

## Examples (from real history)

```
feat(core): add identity module for persistent agent keypairs
feat(agents): add kyc_decide tool and KycWorker/KycVerifier
feat(viewer): surface persistent identity badge on receipts
feat(examples): persist kyc_receipts agent identities to disk
fix(core): correct misleading doc comment on private IdentityFile
fix(compliance): remove dangling identity.rs reference from SR 11-7 mapping
docs(compliance): gtm phase 1 SR 11-7 pre-mapping
docs(plan): add DECISIONS.md with framework comparison and recommendation
test(agents): add phase 4 parallel-verifiers integration test
```

**One logical change per commit.** Don't bundle unrelated fixes — a clippy fix in `core` and a doc fix in `compliance` are two commits, not one.

**Don't commit:** `data/`, `attestations.json`, `executions.json`, `target/` — runtime outputs covered by `.gitignore`. Identity files at `data/identities/*.json` are local secrets and must never be committed.
