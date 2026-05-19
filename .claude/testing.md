# Testing & Quality

## Makefile targets

```bash
make check        # CI gate: lint then test (must pass before merging)
make lint         # cargo fmt --all --check + cargo clippy --workspace -D warnings
make test         # cargo test --workspace (unit + integration)
make test-unit    # cargo test --workspace --lib  (pure unit tests in src/)
make test-int     # cargo test --workspace --tests  (integration tests in tests/)
make fix          # auto-format + apply safe clippy fixes
```

`make check` is the hard gate. Run it before every commit. If it fails, fix before continuing — do not skip it with `--no-verify` or commit-then-fix.

## Test mandate

Every feature commit must include at least one test for the new behaviour. Every bug fix must include a regression test that would have caught the bug. These are not optional — a commit that adds behaviour without a test, or fixes a bug without a regression test, is incomplete.

If a behaviour genuinely cannot be exercised without infrastructure unavailable in tests (e.g. a real network call to `awp-cloud` from `CloudSink`), document why in the PR description and supply a mock-based test instead. This should be rare.

## Two test patterns

**Unit tests** — pure functions. Live in `#[cfg(test)]` modules inside the source file. Import from `super::*`. Use for any logic that does not need agent orchestration, file I/O, or cross-process behaviour.

```rust
// crates/awp-core/src/attestation.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_payload_is_deterministic() {
        let a = Attestation { /* fixed fields */ };
        assert_eq!(a.signing_payload(), a.signing_payload());
    }

    #[test]
    fn canonical_bytes_match_known_vector() {
        let req = KycRequest { customer_id: "4711".into(), amount_cents: 50_00, /* ... */ };
        assert_eq!(req.canonical_bytes(), include_bytes!("../tests/vectors/kyc_4711.bin"));
    }
}
```

**Integration tests** — live in `crates/<crate>/tests/`, one `.rs` file per scenario. Use for anything that crosses module boundaries: Worker/Verifier round-trips, dispatcher coordination, file-store persistence, tamper detection. Each test runs in its own binary, so shared helpers go in `tests/common/mod.rs`.

```rust
// crates/awp-agents/tests/tampered_receipt.rs
#[tokio::test]
async fn verifier_rejects_post_signing_tamper() {
    let worker = KycWorker::with_identity(AgentIdentity::generate("worker"));
    let mut att = worker.decide_and_attest(&request).await.unwrap();
    att.output = serde_json::json!({"decision": "Approve"});  // tamper after signing
    let verdict = KycVerifier::new("verifier").verify_attestation_struct(&att, &request).await;
    assert!(matches!(verdict.status, AttestationStatus::Verified { attestation_valid: false, .. }));
}
```

## Determinism

Tests must be deterministic. If a test depends on time, seed it with a fixed timestamp. If it depends on a keypair, generate from a fixed seed (`AgentIdentity::from_seed(...)` in test code) — never use `AgentIdentity::generate(...)` inside an assertion path. Flaky tests block the gate and are a worse problem than no test at all.

## Byte-identical encodings across languages

When changes touch any of:

- `Attestation::signing_payload`
- `KycRequest::canonical_bytes` (or any new `canonical_bytes` impl)
- The audit viewer's JS verification path
- The PyO3 bindings (`crates/awp-python/`)

the test must assert byte equality against a checked-in test vector, not just signature verification. Signature-verify-only tests can mask canonical-encoding drift that silently breaks cross-language verification in production. The Rust canonical encoding is the source of truth; JS and Python must match it bit-for-bit.

## Clippy rules

Fix the lint, do not silence it. Never silence clippy warnings globally with `#[allow(...)]` at the module or crate level. Per-call-site `#[allow]` with a `// reason: ...` comment is acceptable when the lint is genuinely wrong; the reason must be specific to that call site.
