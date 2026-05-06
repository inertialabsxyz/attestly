# AWP Prototype Development Plan

**Duration:** 8 weeks  
**Commitment:** 2-4 hours/week  
**Framework:** AutoAgents (primary), with evaluation checkpoint  
**Starting Point:** Working Worker-Verifier prototype

---

## Overview

```
Week 1-2          Week 3-4           Week 5-6          Week 7-8
────────────────────────────────────────────────────────────────
│ Attestations │ → │ Dispatcher  │ → │ Batching   │ → │ Evaluate │
│ + Signing    │   │ + Orchestr. │   │ + Merkle   │   │ + Decide │
────────────────────────────────────────────────────────────────
```

---

## Weeks 1-2: Attestations & Signing

### Goal

Worker and Verifier produce signed attestations for every task execution. Verifier uses a tool to cryptographically verify the Worker's attestation before checking answer correctness.

### Verification Flow

```
Worker produces answer + attestation
         │
         ▼
┌─────────────────────────────────────────────────────────────┐
│  Verifier agent:                                            │
│                                                             │
│  1. Call verify_attestation tool ──► crypto check (tool)    │
│  2. Call calculate tool          ──► solve independently    │
│  3. Reason about both results    ──► agree/disagree (LLM)   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
         │
         ▼
   Verifier attestation
   (references worker attestation ID)
   (status: attestation_valid + answer_correct)
```

### Tasks

| Task | Est. Time | Notes |
|------|-----------|-------|
| Define `Attestation` struct | 30 min | Simplified subset of full spec |
| Add `ed25519-dalek` for signing | 30 min | Key generation, sign, verify |
| Generate agent keypairs on startup | 30 min | Ephemeral for now, persist later |
| Worker produces attestation after task | 1-2 hrs | Hook into agent output |
| **Implement `verify_attestation` tool** | 1 hr | Verifier checks Worker's attestation |
| Verifier attestation references worker | 1 hr | Chain of attestations |
| Store attestations to JSON file | 30 min | Simple append-only log |
| Verify attestation signatures on read | 30 min | Round-trip test |

### Data Structures

```rust
struct Attestation {
    id: Uuid,
    agent_id: String,
    agent_pubkey: [u8; 32],
    
    task_hash: [u8; 32],      // SHA256 of task input
    output_hash: [u8; 32],    // SHA256 of output
    output: String,           // Actual output (or URI)
    
    status: AttestationStatus,
    
    // For verifier: reference to what's being verified
    references: Option<Uuid>,
    
    timestamp: i64,
    signature: [u8; 64],
}

enum AttestationStatus {
    Completed,
    Failed(String),
    Verified {
        attestation_valid: bool,  // Crypto check: signature + hashes
        answer_correct: bool,     // Semantic check: is the answer right
    },
}
```

### Verifier Tool: `verify_attestation`

```rust
#[tool(description = "Verify an attestation's cryptographic signature and hash integrity")]
fn verify_attestation(attestation_json: String) -> VerificationResult {
    let attestation: Attestation = serde_json::from_str(&attestation_json)?;
    
    // Check signature
    let pubkey = PublicKey::from_bytes(&attestation.agent_pubkey)?;
    let payload = attestation.signing_payload();
    let sig_valid = pubkey.verify(&payload, &attestation.signature).is_ok();
    
    // Check output hash matches claimed output
    let output_hash = sha256(attestation.output.as_bytes());
    let hash_valid = output_hash == attestation.output_hash;
    
    VerificationResult {
        signature_valid: sig_valid,
        hash_valid: hash_valid,
        overall_valid: sig_valid && hash_valid,
        agent_pubkey: hex::encode(attestation.agent_pubkey),
        timestamp: attestation.timestamp,
    }
}

struct VerificationResult {
    signature_valid: bool,
    hash_valid: bool,
    overall_valid: bool,
    agent_pubkey: String,
    timestamp: i64,
}
```

### Key Questions to Answer

1. How do you cleanly inject attestation generation into AutoAgents output flow?
2. Should attestation be created inside agent tool, or as post-processing?
3. How to handle partial failures (agent runs but attestation signing fails)?
4. How should Verifier behave if attestation is cryptographically invalid but answer is correct?

### Exit Criteria

- [ ] Run worker-verifier example
- [ ] Worker produces signed attestation
- [ ] Verifier calls `verify_attestation` tool to check Worker's attestation
- [ ] Verifier produces attestation with both `attestation_valid` and `answer_correct`
- [ ] Attestations persisted to `attestations.json`
- [ ] Can load and verify signatures independently

---

## Weeks 3-4: Dispatcher & Orchestration

### Goal

Three-agent system where Dispatcher coordinates Worker and Verifier.

### Architecture

```
                    ┌─────────────┐
      task          │             │
    ────────────►   │  Dispatcher │
                    │             │
                    └──────┬──────┘
                           │
            ┌──────────────┼──────────────┐
            │              │              │
            ▼              │              ▼
     ┌────────────┐        │       ┌────────────┐
     │   Worker   │        │       │  Verifier  │
     └─────┬──────┘        │       └─────┬──────┘
           │               │             │
           ▼               │             ▼
    attestation ───────────┴──────► attestation
                                   (references worker)
```

### Tasks

| Task | Est. Time | Notes |
|------|-----------|-------|
| Create Dispatcher agent | 1 hr | Routes tasks, no computation |
| Implement sequential flow | 1-2 hrs | Worker → Verifier |
| Pass worker attestation to verifier | 1 hr | Attestation as context |
| Dispatcher collects both attestations | 1 hr | Aggregation point |
| Handle worker timeout | 1 hr | What happens on failure? |
| Handle verifier disagreement | 30 min | Log, flag, but don't halt |

### Coordination State

```rust
struct TaskExecution {
    task_id: Uuid,
    task_input: String,
    status: ExecutionStatus,
    
    worker_attestation: Option<Attestation>,
    verifier_attestation: Option<Attestation>,
    
    started_at: i64,
    completed_at: Option<i64>,
}

enum ExecutionStatus {
    Pending,
    WorkerRunning,
    WorkerComplete,
    VerifierRunning,
    Complete { 
        attestation_valid: bool,
        answer_correct: bool,
    },
    Failed { stage: String, reason: String },
}
```

### Key Questions to Answer

1. Where does coordination state live - in Dispatcher, or external store?
2. How does AutoAgents handle agent-to-agent communication?
3. What's the cleanest way to pass attestation context to Verifier?

### Exit Criteria

- [ ] Dispatcher receives task, routes to Worker
- [ ] Dispatcher waits for Worker, triggers Verifier
- [ ] Both attestations collected and linked
- [ ] Timeout handling works (Worker takes >30s)
- [ ] Full execution persisted as `TaskExecution`

---

## Weeks 5-6: Attestation Batching

### Goal

Collect attestations into Merkle tree batches with inclusion proofs.

### Architecture

```
attestations (stream)
        │
        ▼
┌───────────────────┐
│      Batcher      │
│                   │
│  buffer: Vec<A>   │──── trigger: time OR count
│                   │
└────────┬──────────┘
         │
         ▼
┌───────────────────┐
│   Merkle Tree     │
│                   │
│     root hash     │──── ready for chain anchoring
│                   │
└────────┬──────────┘
         │
         ▼
┌───────────────────┐
│     SQLite        │
│                   │
│  - batches        │
│  - attestations   │
│  - proofs         │
└───────────────────┘
```

### Tasks

| Task | Est. Time | Notes |
|------|-----------|-------|
| Add `rs_merkle` or implement simple tree | 1 hr | Start with rs_merkle |
| Define `Batch` struct | 30 min | ID, root, attestation refs |
| Batcher service with trigger logic | 1-2 hrs | Time-based + count threshold |
| Generate inclusion proof per attestation | 1 hr | Proof path in tree |
| Verify inclusion proof | 30 min | Given root, verify attestation |
| SQLite schema for batches | 1 hr | Persist batches + proofs |
| Integration: Dispatcher feeds Batcher | 1 hr | Wire it together |

### Data Structures

```rust
struct Batch {
    id: Uuid,
    merkle_root: [u8; 32],
    attestation_ids: Vec<Uuid>,
    attestation_count: u32,
    created_at: i64,
    
    // Populated if/when anchored
    anchor_tx: Option<String>,
    anchor_chain: Option<String>,
}

struct InclusionProof {
    attestation_id: Uuid,
    attestation_hash: [u8; 32],
    batch_id: Uuid,
    proof_path: Vec<ProofNode>,
}

struct ProofNode {
    hash: [u8; 32],
    position: Position, // Left or Right
}
```

### Batcher Configuration

```rust
struct BatcherConfig {
    max_batch_size: u32,        // Trigger if buffer reaches this
    max_batch_age_secs: u64,    // Trigger if oldest attestation exceeds this
    min_batch_size: u32,        // Don't batch fewer than this (unless forced)
}

// Suggested defaults for prototype
BatcherConfig {
    max_batch_size: 10,
    max_batch_age_secs: 60,
    min_batch_size: 1,
}
```

### Key Questions to Answer

1. What's the right batch trigger balance - time vs count?
2. Should proof be stored, or regenerated on demand?
3. How to handle late attestations (batch already created)?

### Exit Criteria

- [ ] Attestations flow into Batcher
- [ ] Batches created on trigger (time or count)
- [ ] Merkle root computed correctly
- [ ] Inclusion proof generated for each attestation
- [ ] Proof verification works given only root + proof + attestation
- [ ] All data persisted to SQLite

---

## Weeks 7-8: Evaluate & Decide

### Goal

Make informed decision on framework and direction for AWP Phase 2.

### Evaluation Tasks

| Task | Est. Time | Notes |
|------|-----------|-------|
| Review pain points log | 30 min | What was hard? |
| Try MCP integration OR parallel workflow | 2-3 hrs | One hard thing |
| Compare: what would this look like in swarms? | 1 hr | Mental exercise |
| Compare: what if thin layer on Rig? | 1 hr | Mental exercise |
| Update AWP spec with learnings | 1-2 hrs | Spec amendments |
| Write DECISIONS.md | 1 hr | Document choices |

### Evaluation Criteria

| Criterion | Weight | AutoAgents | swarms-rs | Custom on Rig |
|-----------|--------|------------|-----------|---------------|
| Attestation integration | High | ? | ? | ? |
| Orchestration flexibility | High | ? | ? | ? |
| Error handling clarity | Medium | ? | ? | ? |
| Documentation quality | Medium | ? | ? | ? |
| Community/maintenance | Low | ? | ? | ? |

Fill in after 6 weeks of experience.

### Decision Options

**Option A: Stay with AutoAgents**
- Pros: Familiarity, working code, actor model fits
- Cons: [TBD based on experience]
- Action: Continue building on prototype

**Option B: Switch to swarms-rs**
- Pros: [TBD]
- Cons: [TBD]
- Action: Port prototype, accept 2-week setback

**Option C: Thin custom layer on Rig**
- Pros: Full control, Rig is mature
- Cons: More work, less multi-agent out of box
- Action: Build minimal orchestration layer

**Option D: Pause and reassess scope**
- Pros: Avoid sunk cost if direction is wrong
- Cons: Lose momentum
- Action: Return to spec, simplify

### Exit Criteria

- [ ] Pain points documented
- [ ] One stretch task completed (MCP or parallel)
- [ ] Comparison notes written
- [ ] Framework decision made with rationale
- [ ] AWP spec updated
- [ ] DECISIONS.md complete

---

## Repo Structure

```
awp-prototype/
├── Cargo.toml
├── README.md
│
├── crates/
│   ├── awp-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── attestation.rs    # Attestation struct, serialization
│   │       ├── signing.rs        # Ed25519 key management
│   │       ├── merkle.rs         # Tree construction, proofs
│   │       └── storage.rs        # SQLite persistence
│   │
│   └── awp-agents/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── tools.rs          # Shared tools (calculate, verify_attestation)
│           ├── worker.rs         # Worker agent
│           ├── verifier.rs       # Verifier agent
│           ├── dispatcher.rs     # Dispatcher/coordinator
│           └── batcher.rs        # Batching service
│
├── examples/
│   ├── simple_attestation.rs     # Week 1-2 checkpoint
│   ├── dispatcher_flow.rs        # Week 3-4 checkpoint
│   └── full_pipeline.rs          # Week 5-6 checkpoint
│
├── docs/
│   ├── SPEC.md                   # AWP specification
│   ├── DECISIONS.md              # Design decision log
│   └── PAIN_POINTS.md            # Framework friction notes
│
└── data/
    └── .gitkeep                  # SQLite db lives here (gitignored)
```

---

## Weekly Log Template

Keep a running log in `docs/DECISIONS.md`:

```markdown
## Week N (dates)

**Hours spent:** X

**Completed:**
- Thing 1
- Thing 2

**Blocked/Deferred:**
- Thing 3 (reason)

**Pain points:**
- Framework issue X
- Unclear how to Y

**Questions raised:**
- Should we Z?

**Next week:**
- Priority 1
- Priority 2
```

---

## Risk Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| AutoAgents doesn't fit | Medium | High | Evaluation checkpoint at week 8 |
| Scope creep | Medium | Medium | Strict phase boundaries |
| Lose momentum | Medium | Medium | Small weekly deliverables |
| Over-engineering | Low | Medium | No chain integration until Phase 2 |
| Framework abandoned | Low | High | Minimal framework coupling in awp-core |

---

## What's Explicitly Deferred

- On-chain anchoring (Phase 2)
- Agent identity registration (Phase 2)
- Multiple LLM providers (not needed yet)
- Peer verification with untrusted agents (using own agents)
- Production error handling (prototype quality acceptable)
- API/HTTP interface (CLI only for now)

---

## Success Criteria (End of 8 Weeks)

1. **Working pipeline:** Task → Dispatcher → Worker → Verifier → Attestations → Batch
2. **Data integrity:** Can verify any attestation's inclusion in batch given only the root
3. **Clear decision:** Know which framework to build on (or build custom)
4. **Updated spec:** AWP spec reflects learnings
5. **Documented friction:** Know what's hard, not just what works

---

## Next Steps After Week 8

Depending on decision:

**If continuing with current approach:**
- Phase 2: On-chain anchoring (choose chain, write contract)
- Phase 3: Agent identity model implementation
- Phase 4: External task submission API

**If pivoting:**
- Re-scope based on learnings
- Potentially simplify (attestations without multi-agent?)
- Revisit timeline

---

*Last updated: 2026-05-06*

---

## Appendix A: LuaI as Execution Layer

### Overview

[LuaI](https://github.com/inertialabsxyz/luai) is a deterministic, sandboxed Lua VM designed for AI agent tool orchestration with ZK-provable execution traces. It provides a two-phase architecture: fast Lua execution for normal operation, with optional zkVM proving (RISC Zero/SP1) when cryptographic verification is required.

Integrating LuaI as the execution layer for AWP agent tools would significantly strengthen the verification model.

### The Verification Problem

The current plan has three verification approaches, each with limitations:

| Mode | How it works | Limitation |
|------|--------------|------------|
| Self-attested | Agent signs its own output | No actual verification |
| Deterministic re-execution | Verifier re-runs computation | Requires trusted re-executor |
| Peer/LLM verification | Another agent checks the work | Non-deterministic, expensive |

**Core issue:** Verification either requires trust or is non-deterministic.

### How LuaI Solves This

LuaI produces **execution traces** - a complete record of every operation performed during execution. This enables:

1. **Deterministic replay** - Anyone can replay the trace and confirm the output
2. **No re-execution trust** - The trace itself is the proof
3. **ZK upgrade path** - Generate succinct proof that trace is valid (no replay needed)

```
┌─────────────────────────────────────────────────────────────┐
│  Current: "Trust me" or "Re-solve it yourself"              │
│                                                             │
│  Task ──► Agent ──► Output ──► Verifier re-solves ──► ✓/✗  │
│                                  (non-deterministic)        │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  With LuaI: "Here's exactly what I did"                     │
│                                                             │
│  Task ──► Agent ──► LuaI ──► Output + Trace                │
│                                    │                        │
│                                    ▼                        │
│                              Verifier replays               │
│                              trace (deterministic) ──► ✓/✗  │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Future: "Here's a proof I did it correctly"                │
│                                                             │
│  Task ──► Agent ──► LuaI ──► Output + ZK Proof             │
│                                    │                        │
│                                    ▼                        │
│                              Verify proof (O(1)) ──► ✓/✗    │
└─────────────────────────────────────────────────────────────┘
```

### Architecture with LuaI

```
┌─────────────────────────────────────────────────────────────┐
│                        Worker Agent                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐      ┌─────────────────────────────────┐  │
│  │   LLM       │      │  LuaI VM                        │  │
│  │  (reasoning)│ ───► │  - Sandboxed execution          │  │
│  │             │      │  - Deterministic                │  │
│  └─────────────┘      │  - Captures execution trace     │  │
│                       └──────────────┬──────────────────┘  │
│                                      │                      │
│                                      ▼                      │
│                       ┌─────────────────────────────────┐  │
│                       │  ExecutionResult                │  │
│                       │  - output: Value                │  │
│                       │  - trace: ExecutionTrace        │  │
│                       │  - trace_hash: [u8; 32]         │  │
│                       └─────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        Attestation                           │
├─────────────────────────────────────────────────────────────┤
│  id: Uuid                                                   │
│  agent_id: String                                           │
│  agent_pubkey: [u8; 32]                                     │
│                                                             │
│  task_hash: [u8; 32]                                        │
│  output_hash: [u8; 32]                                      │
│  trace_hash: [u8; 32]          // NEW: hash of exec trace   │
│  trace_location: Option<URI>   // NEW: where trace stored   │
│                                                             │
│  status: AttestationStatus                                  │
│  verification_mode: VerificationMode                        │
│                                                             │
│  timestamp: i64                                             │
│  signature: [u8; 64]                                        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         Verifier                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. Fetch trace from trace_location                         │
│  2. Verify trace_hash matches                               │
│  3. Replay trace in LuaI VM                                 │
│  4. Confirm replayed output == attested output              │
│  5. (Future: verify ZK proof instead of replay)             │
│                                                             │
│  No LLM reasoning required for verification.                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Updated Data Structures

```rust
struct Attestation {
    id: Uuid,
    agent_id: String,
    agent_pubkey: [u8; 32],
    
    task_hash: [u8; 32],
    output_hash: [u8; 32],
    output: String,
    
    // LuaI execution trace
    trace_hash: [u8; 32],
    trace_location: Option<String>,  // URI to fetch full trace
    
    // Or, for ZK-verified execution
    zk_proof: Option<ZkProof>,
    
    status: AttestationStatus,
    references: Option<Uuid>,
    
    timestamp: i64,
    signature: [u8; 64],
}

enum VerificationMode {
    SelfAttested,
    
    // Verifier replays execution trace in LuaI
    TraceReplay {
        trace_hash: [u8; 32],
    },
    
    // Verifier checks ZK proof (no replay needed)
    ZkVerified {
        proof_system: ProofSystem,  // RISC0, SP1, etc.
        verification_key: [u8; 32],
    },
    
    // Fallback for non-LuaI tasks
    PeerVerified {
        verifier_requirements: VerifierRequirements,
    },
}

enum ProofSystem {
    RiscZero,
    SP1,
}
```

### Tool Implementation Pattern

Instead of native Rust tools, tools are implemented as Lua scripts:

```lua
-- tools/calculate.lua
function calculate(expression)
    -- LuaI provides safe math evaluation
    local result = math.eval(expression)
    return {
        expression = expression,
        result = result
    }
end
```

Worker agent invokes via LuaI:

```rust
// In Worker agent
let script = include_str!("tools/calculate.lua");
let input = json!({ "expression": "140 * 0.60" });

let result = luai.execute(script, "calculate", input)?;
// result.output = { "expression": "140 * 0.60", "result": 84.0 }
// result.trace = ExecutionTrace { ... }
// result.trace_hash = [u8; 32]
```

### Verify Attestation Tool (LuaI Version)

```rust
#[tool(description = "Verify an attestation by replaying its execution trace")]
fn verify_attestation(attestation_json: String) -> VerificationResult {
    let attestation: Attestation = serde_json::from_str(&attestation_json)?;
    
    // 1. Check signature
    let sig_valid = verify_signature(&attestation);
    
    // 2. Fetch and verify trace
    let trace = fetch_trace(&attestation.trace_location)?;
    let trace_hash_valid = sha256(&trace) == attestation.trace_hash;
    
    // 3. Replay trace in LuaI
    let replay_result = luai.replay_trace(&trace)?;
    let output_matches = sha256(&replay_result.output) == attestation.output_hash;
    
    VerificationResult {
        signature_valid: sig_valid,
        trace_hash_valid,
        replay_successful: true,
        output_matches,
        overall_valid: sig_valid && trace_hash_valid && output_matches,
    }
}
```

### Integration Timeline Options

**Option 1: Integrate from Week 1**

| Week | Scope |
|------|-------|
| 1-2 | Worker executes tools via LuaI, attestations include trace_hash |
| 3-4 | Verifier replays traces (no LLM verification needed) |
| 5-6 | Batching (unchanged) |
| 7-8 | Evaluate, potentially add ZK proof generation |

**Option 2: Add in Phase 2**

| Phase | Scope |
|-------|-------|
| Phase 1 (current) | Complete plan as-is with LLM-based verification |
| Phase 2 | Swap tool execution to LuaI, add trace-based verification |
| Phase 3 | Add ZK proof generation and on-chain verification |

**Option 3: Parallel Track**

Run both approaches:
- Simple tasks: LuaI execution with trace verification
- Complex tasks: LLM execution with peer verification

This acknowledges that not all agent work can be expressed as Lua scripts.

### What Changes

| Component | Without LuaI | With LuaI |
|-----------|--------------|-----------|
| Tool execution | Native Rust | Lua scripts in LuaI VM |
| Verification | LLM re-solves or trusts | Trace replay (deterministic) |
| Verifier role | Semantic checking (LLM) | Mechanical verification (no LLM) |
| Attestation size | Small | Larger (includes trace hash) |
| Storage needs | Output only | Output + traces |
| ZK readiness | Not addressed | Native path |

### Limitations

1. **Not all tasks fit** - Complex reasoning, subjective judgment, or tasks requiring external state can't be expressed as pure Lua computations

2. **Trace storage** - Traces can be large; need storage strategy (store hash on-chain, full trace off-chain)

3. **Tool conversion** - Existing native tools need Lua equivalents

4. **LuaI maturity** - Depends on LuaI's current state and stability

### Recommendation

Start with the current plan (LLM-based verification) but **design attestation structure to accommodate traces**. This means:

1. Include `trace_hash` and `trace_location` fields now (optional/nullable)
2. Implement LuaI integration for one simple tool (calculator) as a proof of concept
3. Evaluate in Week 7-8 whether to commit to LuaI as primary execution layer

This keeps momentum while preserving the upgrade path.

### References

- LuaI repository: https://github.com/inertialabsxyz/luai
- ZK proof systems: RISC Zero, SP1
- Execution trace design: [link to LuaI docs if available]
