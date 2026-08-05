# Agent Prompts — Attestly Production Checklist

## Step 3 — Phase 3: Persistent agent identity — restart regression test

**Branch:** `phase/3-identity-regression-test`
**Depends on:** _(none for correctness — branch from `main`. Runs in parallel
with Step 2.)_

**Runs in parallel with:** Step 2 (production hardening). Step 2 is entirely
under `services/attestly-cloud/`; you are entirely under
`python/attestly-langgraph/` + `crates/attestly-python/`. You share no files —
do not coordinate.

**Prompt:**

You are implementing the **restart regression test** for persistent agent
identity. The full specification is in
[`docs/PRODUCTION_CHECKLIST.md`](../docs/PRODUCTION_CHECKLIST.md) under **§3
Persistent agent identity (Persona-B load-bearing)**. Read that section
carefully first.

§3's substance is **already proven manually** and marked `[x]`: the SDK default
path yields a stable pubkey across separate processes, and that exact pubkey
lands in the cloud-stored receipt. The **one remaining, unchecked item** is:

> Add a regression test asserting identity survives a restart (guards the
> `load_or_create` default against a future swap to `generate()`). This is the
> only remaining work on this item — the substance is already proven.

That is your entire task. Do not re-architect identity, do not add cloud calls —
guard the default so a future refactor can't silently swap `load_or_create` for
the ephemeral `generate()`.

### Context

Concrete current state (verified in the repo):

- **The SDK default path** is in `python/attestly-langgraph/attestly/langgraph/wrapper.py`.
  `attest()` computes `ids_dir` (default `Path("data")/"identities"`,
  `wrapper.py:58`; overridable via the `identities_dir` kwarg) and calls
  **`_attestly_core.AgentIdentity.load_or_create(str(ids_dir), agent_id)`**
  (`wrapper.py:120`; and again at `:123-125` for the verifier). `_attestly_core`
  is `from attestly import _native` (`wrapper.py:39-46`). An explicit
  `identity=` kwarg short-circuits the file-store lookup (`identity or ...`).
- **The PyO3 binding** `AgentIdentity.load_or_create(path: str, agent_id: str)`
  (`crates/attestly-python/src/lib.rs:104-110`) builds a
  `FileIdentityStore::new(PathBuf::from(path))` and calls the core
  `AgentIdentity::load_or_create(&store, agent_id)`. It writes one JSON file per
  agent at `<dir>/<agent_id>.json` (`IdentityFile`: hex `secret_key` +
  `public_key`, `0600` on Unix). `AgentIdentity.public_key` returns the 32-byte
  verifying key; `AgentIdentity.generate(agent_id)` is the **ephemeral, non-persisted**
  path this test must guard against.
- **The exact seam the checklist names:** if someone changes `wrapper.py:120`
  from `load_or_create(str(ids_dir), agent_id)` to
  `AgentIdentity.generate(agent_id)`, keys would rotate every process — a
  settlement system can't trust that. Your test must **fail** if that swap
  happens.
- **Existing identity coverage (none crosses a process boundary):**
  `crates/attestly-python/tests/test_api.py::test_load_or_create_persists_across_calls`
  (`:32-39`) is **same-process, repeated calls**. Rust
  `identity.rs` has same-process reload tests. The only `subprocess` use in the
  Python tests is `crates/attestly-python/tests/cross_language.py`, which spawns
  the `attestly-verify` **binary** for the cross-language encoding contract — it
  does **not** test identity reload across processes. **No cross-process /
  cross-restart identity test exists yet.**
- **Pytest wiring:** the root `Makefile` runs `check-python` →
  `python-build python-test python-test-langgraph`. `python-build` creates
  `.venv`, `pip install maturin pytest langgraph`, and `maturin develop
  --release` for `crates/attestly-python`. `python-test` runs
  `pytest crates/attestly-python/tests`; `python-test-langgraph` does
  `pip install --no-deps -e python/attestly-langgraph` then
  `pytest python/attestly-langgraph/tests`. `cargo test` **excludes**
  `attestly-python` (cdylib link issue) — the bindings are covered via pytest.

### Your Task

Add a regression test that proves identity **survives a restart** — i.e. is
stable across **separate OS processes** using the SDK's default `load_or_create`
path, and would break if that path were swapped to `generate()`. One logical
commit.

1. **Write a cross-process test.** Prefer the layer that most directly guards
   the checklist's named seam. Two acceptable placements — pick one, justify in
   the commit body:
   - **Binding-level (recommended, most robust):** in
     `crates/attestly-python/tests/`, a test that spawns a **fresh interpreter**
     twice via `subprocess` (mirror the pattern in `cross_language.py`), each
     running a tiny snippet that does
     `AgentIdentity.load_or_create(<shared tmp dir>, "restart-agent").public_key`
     and prints the hex pubkey. Assert the two processes print the **same**
     pubkey, and that a **different `agent_id`** in a third process prints a
     **different** pubkey. Use a `tmp_path` dir so nothing touches the real
     `data/identities`. Because each pubkey comes from a **new process**, a swap
     to `generate()` (ephemeral) makes the two differ and the test fails — which
     is exactly the guard required.
   - **SDK-level:** in `python/attestly-langgraph/tests/`, a test that drives
     `attest()` (or the `load_or_create` call it makes) across two subprocesses
     with a shared `identities_dir=tmp_path`, asserting the emitted
     attestation's `agent_pubkey` is identical across the two runs. This guards
     `wrapper.py:120` most directly but is heavier (needs a minimal `StateGraph`).
     If you choose this, keep the graph minimal and `pytest.importorskip("langgraph")`.

2. **Make it deterministic and hermetic.** Never assert against
   `AgentIdentity.generate(...)` output (ephemeral, non-deterministic). Use a
   `tmp_path`-scoped identities dir so the test never reads or writes the real
   `data/identities/` (identity files are local secrets, never committed). Spawn
   subprocesses with `sys.executable` and the same venv so the `attestly._native`
   extension resolves. Keep it fast — two or three short subprocess runs.

3. **Add a comment** on the test tying it to the checklist item — one line, e.g.
   `# Guards docs/PRODUCTION_CHECKLIST.md §3: load_or_create must survive a
   restart; a swap to generate() rotates keys and this test fails.` So a future
   reader knows why deleting it is not free.

Wire nothing new into the Makefile — the test must be picked up by the existing
`python-test` (bindings) or `python-test-langgraph` (SDK) pytest invocation
depending on where you place it. Use the `sdk` scope if the test lands under
`python/attestly-langgraph/`, else `python` for the bindings crate.

### Do Not Touch

- `services/attestly-cloud/` — **Step 2's tree**, running in parallel. Zero
  edits there.
- `crates/attestly-core/src/identity.rs` production code and
  `crates/attestly-python/src/lib.rs` production code — the substance is proven;
  you are adding a **test**, not changing `load_or_create`, `generate`, or the
  binding surface. (Adding a test file is fine; editing the `.rs` source is not.)
- `python/attestly-langgraph/attestly/langgraph/wrapper.py` production code —
  do not change the default path; test it as-is.
- The real `data/identities/` directory — never read, write, or commit it.
- On-chain anchoring, cloud-side identity, KMS store — **out of scope** for this
  item; the checklist marks §3 substance done.

### Closing the Loop

When implementation is complete and `make check` passes:

1. Spawn the review agent per [`.claude/review-gate.md`](../.claude/review-gate.md)
   against **`docs/PRODUCTION_CHECKLIST.md` §3 Persistent agent identity** — the
   single unchecked item: "regression test asserting identity survives a
   restart." The review agent should confirm the test **fails** if
   `load_or_create` were swapped for `generate()` (have it reason about, or
   temporarily verify, the guard actually bites).
2. Capture the review agent's structured report.
3. Open a draft PR per [`.claude/pull-requests.md`](../.claude/pull-requests.md)
   (target `main`, title per [`.claude/commits.md`](../.claude/commits.md), e.g.
   `test(python): assert agent identity survives a restart`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline`
   with the review report.

### Verification

```bash
make check
# → passes (check-python runs the new test via python-test or python-test-langgraph)

# Run just the new test directly (binding-level example):
.venv/bin/python -m pytest crates/attestly-python/tests -k restart -v
# → passes: two fresh processes using load_or_create print the SAME pubkey;
#   a different agent_id prints a DIFFERENT pubkey

# Prove the guard bites (do NOT commit this): temporarily edit the test's
# snippet to call AgentIdentity.generate(agent_id) instead of load_or_create,
# rerun the test → it FAILS (pubkeys differ across processes). Revert.
```
