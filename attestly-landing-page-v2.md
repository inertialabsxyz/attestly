# Attestly

## Signed attestations for AI agent work.

Attestly is a minimal protocol for AI agents to produce cryptographically signed attestations of completed work, with optional on-chain anchoring for coordination and settlement.

---

## The Problem

AI agents are doing real work—but there's no standard way to know what they did.

- Who performed this task?
- What exactly was the output?
- When did it happen?
- Can I prove it to a third party?

When agents work on your behalf, you need receipts—not just results.

---

## What Attestly Provides

### Signed Attestations

Every task produces a cryptographically signed attestation linking the agent's identity, the task, and the output.

```
Attestation {
    agent_id + public_key
    task_hash
    output_hash
    status
    timestamp
    signature
}
```

If an agent claims it did the work, the signature proves it.

---

### Multi-Agent Verification

Agents can verify each other's work. A Worker agent completes a task. A Verifier agent checks it independently.

```
Task → Worker → Attestation
                    ↓
              Verifier → Verification Attestation
                              (agrees / disagrees)
```

Both attestations are signed. Disagreements are visible. Trust is earned, not assumed.

---

### On-Chain Anchoring

Attestations batch into Merkle trees. Roots anchor on-chain.

- **Inclusion proofs** verify any attestation against the root
- **Minimal on-chain footprint** — one transaction per batch, not per task
- **Chain-agnostic** — L1, L2, or app-specific chain

The chain becomes a coordination layer, not an execution engine.

---

## How It Works

```
┌─────────────┐     task      ┌─────────────┐
│  Requester  │ ────────────► │   Worker    │
└─────────────┘               └──────┬──────┘
                                     │
                                     ▼
                              ┌─────────────┐
                              │   Execute   │
                              │   + Sign    │
                              └──────┬──────┘
                                     │
                                     ▼
                              ┌─────────────┐
                              │ Attestation │
                              └──────┬──────┘
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                      │
              ▼                      ▼                      ▼
       ┌────────────┐         ┌────────────┐         ┌────────────┐
       │  Verifier  │         │   Batcher  │         │  Storage   │
       │  (checks)  │         │  (merkle)  │         │ (off-chain)│
       └─────┬──────┘         └─────┬──────┘         └────────────┘
             │                      │
             ▼                      ▼
      Verification            ┌────────────┐
      Attestation             │   Chain    │
                              │  (anchor)  │
                              └────────────┘
```

---

## Design Principles

**Blockchain only where necessary.**  
Computation stays off-chain. The chain handles coordination and settlement.

**No abstraction without example.**  
Every protocol element maps to a concrete use case.

**Graceful degradation.**  
System remains functional when the chain is unavailable.

**Identity over keys.**  
An agent's identity is more than its signing key. Versions, lineage, and accountability matter.

---

## Use Cases

### Auditable Task Completion

Agent completes a task. Attestation proves who did it, when, and what the output was. Useful for compliance, billing, or dispute resolution.

### Multi-Agent Workflows

Worker agents produce attestations. Verifier agents check them. Dispatchers coordinate. Every step is signed and linked.

### Cross-Party Coordination

Multiple parties can verify attestations against a shared on-chain root without trusting each other or a central server.

### Settlement for Agent Services

Agent completes task → attestation anchors on-chain → payment releases. Clear proof of work completion.

---

## What Attestly Is Not

- **Not a token.** No incentive theater.
- **Not a DAO.** Agents don't vote.
- **Not on-chain AI.** Execution stays off-chain where it belongs.
- **Not a reputation score.** Task-scoped attestations, not global rankings.

---

## Architecture

### Agent Identity

```rust
AgentIdentity {
    id: UUID,              // Stable across key rotations
    active_key: PublicKey, // Current signing key
    parent: Option<...>,   // Previous version (if upgraded)
    operator: Option<...>, // Human/org accountability
}
```

Key rotation preserves identity. Version upgrades create lineage. Accountability traces to operators.

### Attestation

```rust
Attestation {
    id: UUID,
    agent_id: AgentIdentityRef,
    
    task_hash: Hash,
    output_hash: Hash,
    output: String,
    
    status: Completed | Failed | Verified { 
        attestation_valid: bool,
        answer_correct: bool 
    },
    
    references: Option<UUID>,  // Links to attestation being verified
    
    timestamp: i64,
    signature: Signature,
}
```

Self-contained proof of work. Verifier attestations reference what they're verifying.

### On-Chain Registry

```
anchor(merkle_root, batch_id, timestamp) → AnchorId
verify_inclusion(attestation_hash, proof, anchor_id) → bool
```

Minimal contract. Stores roots, verifies inclusion. Nothing else.

---

## Built With

**[AutoAgents](https://github.com/liquidos-ai/AutoAgents)** — Multi-agent framework in Rust with actor-based coordination.

**Ed25519** — Fast, secure signatures for attestations.

**Merkle Trees** — Efficient batching and inclusion proofs.

---

## Status

Attestly is in active development.

- [x] Specification complete
- [x] Multi-agent prototype (Worker + Verifier)
- [ ] Attestation signing and verification
- [ ] Dispatcher orchestration
- [ ] Merkle batching
- [ ] On-chain anchoring

---

## Get Involved

**GitHub:** [github.com/inertialabsxyz/attestly](https://github.com/inertialabsxyz/attestly)

**Specification:** [Read the spec →](./spec)

**Built by:** [Inertia Labs](https://inertialabs.xyz)

---

## FAQ

**Why not just use signatures?**

Signatures prove *who* signed. Attestly structures *what* is signed—task, output, timing, and references—so attestations are meaningful and composable.

**Why multi-agent verification?**

A single agent's attestation is only as trustworthy as that agent. Having a second agent independently verify creates accountability without requiring a central authority.

**Why anchor on-chain?**

Coordination. Multiple parties can verify attestations against a shared root without trusting each other or a central server.

**Which chains are supported?**

Attestly is chain-agnostic. The reference implementation targets EVM L2s for low-cost anchoring.

**How does this relate to MCP?**

MCP (Model Context Protocol) defines how agents access tools. Attestly defines how agents attest to using them. They're complementary.

**Is this production-ready?**

No. Attestly is a working prototype exploring attestations for agent work. Use it to learn, not to ship.

---

<footer>

*Built for systems that won't be embarrassing in five years.*

</footer>
