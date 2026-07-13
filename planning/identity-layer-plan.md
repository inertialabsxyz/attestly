# AWP Identity Layer Plan

**Status:** Design accepted; v1 scope decisions locked (see §9). Not yet
scheduled — the next step is to turn §9 into implementation issues in the
#16–#24 format. In the same spirit as
[`awp-prototype-plan.md`](awp-prototype-plan.md) and
[`docs/DECISIONS.md`](../docs/DECISIONS.md). No code has been written against it.

**Problem in one sentence:** an AWP attestation today proves *"the holder of
this keypair signed this claim"* — it does **not** prove *who* that holder is,
because `agent_pubkey` is bound to no real-world principal. This plan closes
that gap without turning AWP into an identity provider.

---

## 1. Guiding principle — plug in, don't own

The market research is explicit about the boundary. `awp-market-research.md`
§1.3 places "Agent identity / DIDs" (cheqd, Disco, Privado, World ID for
agents, plus Auth0/Okta agent-identity products) in the **complementary**
column, with the note: *"AWP needs identity, but doesn't need to own it."*
Persona A — the revenue-bearing regulated buyer — wants *"signing keys
controlled by them"* (§2.1).

Two consequences drive every decision below:

1. **AWP owns the *binding format and its verification*, not the *issuer*.**
   We define a portable, signed record that says "key K belongs to principal
   P, per issuer I," and we verify the chain. The issuer can be an org's root
   key, an Okta/Auth0 agent identity, a DID method, or a corporate CA — AWP
   stays agnostic.
2. **Do not build a registry, CA, or token.** Building our own identity
   system re-triggers the "this is crypto infra" positioning problem
   (`awp-market-research.md` §2.3) and competes with well-funded incumbents.
   A hosted *directory* of bindings is an optional convenience, never the
   root of trust.

---

## 2. What exists today (the seam we build on)

Concrete, from the current code:

- **`Attestation`** (`crates/awp-core/src/attestation.rs`) already carries
  `agent_id: String`, `agent_pubkey: [u8; 32]`, and `signature: [u8; 64]`.
  Verifiers already chain claims via `references: Option<Uuid>`.
- **`AgentIdentity` / `IdentityStore`** (`crates/awp-core/src/identity.rs`).
  `IdentityStore` is a two-method trait (`load` / `save`) — the *storage*
  seam. `FileIdentityStore` is the only impl (plaintext, dev-only). Managed
  storage is tracked separately in issue #19.
- **Hosted account binding** — `services/awp-cloud` authenticates ingest by
  API key → `AuthedAccount`, so the cloud already knows *"account X submitted
  this receipt."* This is a service-layer binding that does not survive the
  receipt leaving the hosted DB.
- **Offline verification** — `tools/audit-viewer` reconstructs the signing
  payload and verifies ed25519 in-browser with no network. Any identity
  binding we add **must** be verifiable by this same offline path, or it
  isn't worth the bytes.

**The missing piece is orthogonal to all of the above:** a verifiable
statement binding `agent_pubkey` to a principal. Storage (#19) answers *where
the key lives*; this plan answers *whom the key represents*.

---

## 3. Trust levels — stage them, don't build all at once

### Level 0 — Self-signed (today)

`agent_pubkey` is self-asserted. Provides non-repudiation ("this key signed
this") but no accountability ("this org stands behind it"). The prototype
default even mints a fresh key per run (`AgentIdentity::generate`). Keep this
as the zero-config local-dev path forever.

### Level 1 — Account binding (mostly already there)

Surface the hosted service's existing account→attestation link explicitly:
the cloud can state "these public keys belong to account X." Cheap, but
service-dependent and non-cryptographic — it evaporates offline. Useful as a
directory convenience (Level 3), **not** as a root of trust.

### Level 2 — Registration record / key attestation (**build this first**)

A signed, portable record binding a key to a principal:

> Issuer I, at time T, authorizes key K to act as agent A (optionally: until
> expiry E, for scope S).

AWP defines the record's canonical format and verifies the signature chain
from `agent_pubkey` up to an issuer key the relying party already trusts. The
issuer is pluggable (org root key, DID, Okta/Auth0, corporate CA). This is
the "own the format, plug the issuer" play, and it composes directly with the
existing `IdentityStore` seam and the deferred "identity registration"
milestone. **Everything below specs Level 2.**

### Level 3 — Directory + full DID/VC resolution (later, per design partner)

Optional hosted directory that indexes bindings for discovery, and resolvers
that fetch an issuer's key/policy from a DID document or a verifiable
credential (cheqd, World ID, etc.). Build a specific resolver only when a
specific partner needs a specific method — never speculatively.

---

## 4. Level 2 spec — the `IdentityBinding` record

### 4.1 Wire shape

A new type in `awp-core` (name illustrative), canonicalized and signed the
same way `Attestation` is (serde field order + compact JSON — see
`docs/DECISIONS.md` on the canonicalization convention; the same
float/non-BMP caveats apply and should be pinned by a cross-language vector):

```rust
pub struct IdentityBinding {
    pub id: Uuid,
    /// The agent key being vouched for — matches Attestation.agent_pubkey.
    pub subject_pubkey: [u8; 32],
    /// Human-facing agent id this key is authorized to use.
    pub subject_agent_id: String,
    /// Who is making the assertion (see IssuerRef below).
    pub issuer: IssuerRef,
    /// Optional scope hints (e.g. task classes this key may attest for).
    pub scope: Option<String>,
    pub issued_at: i64,
    pub expires_at: Option<i64>,
    /// Issuer's signature over the canonical binding payload.
    pub issuer_pubkey: [u8; 32],
    pub signature: [u8; 64],
}

pub enum IssuerRef {
    /// Raw ed25519 issuer key — trust anchored out-of-band (the relying
    /// party already knows this org key). Simplest; ship first.
    Key,
    /// DID that resolves to the issuer's verification method.
    Did(String),
    /// Opaque handle into an external IdP (Okta/Auth0 agent identity),
    /// resolved by a pluggable resolver.
    External { provider: String, ref_: String },
}
```

### 4.2 Verification semantics (the load-bearing part)

`verify_binding(binding, trusted_issuers) -> BindingVerdict` must:

1. Verify `binding.signature` against `binding.issuer_pubkey` over the
   canonical payload (pure ed25519 — works offline).
2. Confirm `binding.issuer_pubkey` resolves to a **trusted issuer** for the
   relying party. For `IssuerRef::Key`, that means the key is in the relying
   party's trust set. For `Did`/`External`, a resolver maps the issuer
   reference to a verification key/policy (Level 3).
3. Check `expires_at` / `issued_at` sanity.
4. Return a structured verdict: `Trusted { issuer, scope }`,
   `UntrustedIssuer`, `Expired`, or `SignatureInvalid` — **never a bare
   bool.** (Mirrors the Verifier's `attestation_valid`/`answer_correct`
   split so downstream consumers can reason, not just gate.)

An `Attestation` is then judged *accountable* when: its signature verifies
**and** a valid `IdentityBinding` chains its `agent_pubkey` to a trusted
issuer. The two checks stay independent — a receipt with no binding is still
a valid Level-0 receipt, just not an accountable one.

### 4.3 Where bindings travel

A binding is issued **once** and reused across thousands of attestations from
the same key, so it lives on its **own channel** — never inlined per
attestation (that would duplicate it massively and couple two different
lifecycles). This mirrors the id-reference/join pattern already used for
executions↔attestations (`docs/DECISIONS.md` D2.6). Concretely:

- A sidecar **`bindings.jsonl`** next to the attestation stream for the
  `FileSink` path.
- A dedicated **`/v1/bindings`** endpoint + table in `services/awp-cloud`,
  joined to attestations at read time, so a share-link can render "signed by
  key K, vouched for by Org O."
- A relying party resolves-and-caches the binding for a given `agent_pubkey`
  once, rather than re-reading it per receipt.
- Verified by `tools/audit-viewer` with the **same** offline ed25519 path —
  the viewer loads bindings from the sidecar and renders the issuer chain per
  row. This is a hard requirement, not a nice-to-have.

---

## 5. Verifier / Worker / viewer integration

- **Worker** — unchanged in how it signs; optionally *presents* its binding
  alongside its attestation (the binding is issued once, out of band, not per
  task).
- **Verifier** — extend the judgment to include binding status: a receipt
  from an unbound or untrusted-issuer key can be flagged even when the
  signature is valid. This is exactly the "disagreement is data, not a halt"
  posture from `docs/DECISIONS.md` D2.4 — surface it, don't suppress it.
- **audit-viewer** — render the issuer chain per row; the existing
  self-test-on-load should gain a binding-verification vector.
- **CloudSink / hosted** — store and index bindings; expose issuer info on
  the dashboard and share pages.

---

## 6. First issuer integrations to support

Ordered by pragmatism, ship strictly in this order and only as far as demand
pulls:

1. **`IssuerRef::Key` (raw org root key).** Zero external dependencies. An
   org generates one issuer keypair, signs bindings for its agents, and
   distributes its public issuer key to relying parties out of band. This
   alone makes receipts accountable for a single-org design partner.
2. **Corporate IdP via `External` (Okta / Auth0 agent identity).** Named as
   complementary in the market research; the most likely enterprise ask.
   Resolver maps an IdP handle to a verification key.
3. **DID method** (one, chosen by the first partner that needs it — likely a
   crypto-native Persona-C network). Do not implement a DID resolver on spec.

---

## 7. Explicit non-goals

- **No AWP-run CA or root of trust.** Relying parties choose their trusted
  issuers. AWP ships verification, not authority.
- **No token, no on-chain identity registry.** Consistent with
  `awp-market-research.md` Appendix A (token is "when, not if" — and not now)
  and the deferred-anchoring posture. A binding is an off-chain signed record.
- **No forced identity for local dev.** Level 0 stays the zero-config path;
  bindings are opt-in.
- **No new dependency without an ask.** Per repo policy — DID/IdP client
  libraries get raised before they land.

---

## 8. Relationship to existing work

- **Issue #19 (managed `IdentityStore`)** answers *where the signing key
  lives* (KMS/keychain/encrypted-at-rest). This plan answers *whom the key
  represents*. They share the identity seam and should be one workstream, not
  two — do #19 first (a safely-held key is a precondition for a key worth
  vouching for), then Level 2 bindings on top.
- **Sequencing vs. the shipping blockers.** This is the deepest gap in the
  *trust story*, but it is **not** a blocker for a design-partner alpha — the
  P1 operational blockers (#16 key delivery, #17 store tests, #18 blob
  durability) are. Recommended order: ship the alpha on the blockers, then
  run the identity workstream (#19 → Level 2) as the feature that upgrades
  receipts from "signed bytes" to "accountable receipts."

---

## 9. Resolved decisions (v1)

The four scoping questions are settled as follows. Each names the deferred
work and its trigger so nothing is silently dropped.

1. **Level 2 scope — `IssuerRef::Key` only.** The `IssuerRef` enum ships with
   all three variants defined (`Key`, `Did`, `External`) so the shape is
   future-proof, but only the raw org-root-key path is *implemented* in v1.
   This fully solves the single-org design-partner case (an org signs
   bindings with one issuer key and distributes the public half out of band)
   with zero external dependencies.
   *Deferred:* `External` (Okta/Auth0) and `Did` resolvers.
   *Trigger:* a named partner requiring that specific issuer.

2. **Canonicalization — keep the serde-order convention; pin a cross-language
   binding vector; constrain the string fields.** `IdentityBinding` is a
   fixed-shape struct we control, with no float fields, so the main
   canonicalization hazard does not arise. The residual risk (non-BMP Unicode
   in `subject_agent_id`/`scope`) is covered by a pinned cross-language vector
   (mirroring `crates/awp-core/tests/cross_language_vector.rs`) plus
   boundary validation/normalization of those string fields.
   *Deferred:* RFC 8785 JCS adoption.
   *Trigger:* when external parties sign **arbitrary/free-form** payloads
   (not the case at L2). See the `docs/DECISIONS.md` canonicalization caveat.

3. **Transport — separate channel** (`bindings.jsonl` sidecar + `/v1/bindings`
   endpoint, joined at read time). Bindings are never inlined per
   attestation. Rationale and mechanics in §4.3.

4. **Revocation — expiry-based only in v1.** `expires_at` is mandatory with a
   recommended short maximum (≤90 days); revocation is "let it expire and
   reissue." This keeps offline verification (the audit-viewer) fully
   self-contained with no network dependency.
   *Deferred:* a hosted revocation signal (a "revoked" list on the cloud,
   for the online path only; offline stays expiry-based).
   *Trigger:* the first regulated partner with a hard key-compromise-response
   SLA that short expiry cannot satisfy.

**Net v1 shape:** binding record + `Key`-issuer verification, serde-canonical
with a pinned cross-language vector, transported on a separate
`bindings.jsonl`/endpoint, expiry-based revocation. Minimal,
offline-verifiable, single-org-accountable — every deferred piece (Okta, DID,
JCS, revocation lists) has a named trigger.

**Next step:** turn this section into a set of issues in the same format as
the shipping-blocker backlog (#16–#24), sequenced after issue #19 (managed
`IdentityStore`) per §8.
