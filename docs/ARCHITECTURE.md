# AWP Prototype Architecture

A description of what is currently shipped in this repository at the close of the 8-week prototype (Phases 1–4 merged to `main`). Diagrams are Mermaid and render inline in GitHub.

This document is descriptive, not prescriptive. For the *why* behind the shape, see [`DECISIONS.md`](DECISIONS.md). For known friction, see [`PAIN_POINTS.md`](PAIN_POINTS.md). For the original plan plus the postmortem, see [`../planning/awp-prototype-plan.md`](../planning/awp-prototype-plan.md).

## Workspace layout

```
awp/
├── crates/
│   ├── awp-core/          # framework-agnostic data + crypto + storage
│   │   ├── attestation.rs # Attestation, AttestationStatus, signing_payload
│   │   ├── signing.rs     # AgentKeypair (ed25519)
│   │   ├── task.rs        # TaskExecution, ExecutionStatus
│   │   ├── execution.rs   # JSONL persistence for executions+attestations
│   │   ├── merkle.rs      # Batch, InclusionProof, attestation_leaf_hash
│   │   └── storage.rs     # SQLite: batches, attestations, proofs
│   ├── awp-agents/        # agent loop + coordination
│   │   ├── worker.rs      # WorkerAgent trait + Worker
│   │   ├── verifier.rs    # VerifierAgent trait + Verifier
│   │   ├── dispatcher.rs  # Dispatcher (single verifier)
│   │   ├── parallel.rs    # ParallelDispatcher (N verifiers)
│   │   ├── batcher.rs     # Batcher (count + time triggered)
│   │   └── tools.rs       # calculate, verify_attestation
│   └── awp-examples/      # shared example helpers
├── examples/
│   ├── simple_attestation.rs
│   ├── dispatcher_flow.rs
│   ├── full_pipeline.rs
│   └── parallel_verifiers.rs
└── data/                  # JSONL logs + SQLite, gitignored
```

`awp-core` has zero dependency on `awp-agents` — the plan's "minimal framework coupling in awp-core" risk-mitigation row was honoured. A future framework swap (see DECISIONS.md) would touch `awp-agents` only.

## High-level architecture

```mermaid
flowchart LR
    subgraph awp-agents
        W[Worker]
        V[Verifier]
        D[Dispatcher]
        PD[ParallelDispatcher]
        B[Batcher]
        T[Tools<br/>calculate<br/>verify_attestation]
    end

    subgraph awp-core
        A[Attestation<br/>+ AgentKeypair]
        TE[TaskExecution<br/>+ ExecutionStatus]
        M[Merkle<br/>Batch + InclusionProof]
        EX[execution.rs<br/>JSONL persistence]
        ST[storage.rs<br/>SQLite]
    end

    subgraph data
        J1[(attestations.json)]
        J2[(executions.json)]
        DB[(awp.db)]
    end

    D --> W & V
    PD --> W
    PD -->|N concurrent| V
    W & V --> T
    D & PD --> EX
    D & PD -.->|optional| B
    EX --> J1 & J2
    B --> M & ST
    ST --> DB

    W & V -. produces .-> A
    D & PD -. owns lifecycle of .-> TE
    B -. seals batch of .-> A
```

### Module dependency rules

- `awp-core` is leaf. No dependency on agents, tokio runtime, or any framework.
- `awp-agents` depends on `awp-core` only. Tokio enters here.
- `awp-examples` depends on both. The four example binaries live in `examples/` and depend on `awp-examples` for shared helpers.

## Core data model

`Attestation` (in `awp-core`) is the central record. Every Worker run, every Verifier run, and every persisted execution composes from it.

```mermaid
classDiagram
    class Attestation {
        +Uuid id
        +String agent_id
        +[u8;32] agent_pubkey
        +[u8;32] task_hash
        +[u8;32] output_hash
        +String output
        +AttestationStatus status
        +Option~Uuid~ references
        +i64 timestamp
        +[u8;64] signature
        +signing_payload() Vec~u8~
    }

    class AttestationStatus {
        <<enumeration>>
        Completed
        Failed(reason)
        Verified(attestation_valid, answer_correct)
    }

    class AgentKeypair {
        +generate() Self
        +public_bytes() [u8;32]
        +sign_attestation(att)
    }

    class TaskExecution {
        +Uuid task_id
        +String task_input
        +ExecutionStatus status
        +Option~Attestation~ worker_attestation
        +Option~Attestation~ verifier_attestation
        +i64 started_at
        +Option~i64~ completed_at
    }

    class ExecutionStatus {
        <<enumeration>>
        Pending
        WorkerRunning
        WorkerComplete
        VerifierRunning
        Complete(attestation_valid, answer_correct)
        Failed(stage, reason)
    }

    class Batch {
        +Uuid id
        +[u8;32] merkle_root
        +Vec~Uuid~ attestation_ids
        +u32 attestation_count
        +i64 created_at
        +Option~String~ anchor_tx
    }

    class InclusionProof {
        +Uuid attestation_id
        +[u8;32] attestation_hash
        +Uuid batch_id
        +Vec~ProofNode~ proof_path
    }

    Attestation --> AttestationStatus
    Attestation ..> AgentKeypair : signed by
    TaskExecution --> ExecutionStatus
    TaskExecution o--> Attestation : 0..2
    Batch o--> Attestation : N (by id)
    InclusionProof ..> Batch : witnesses
```

### How attestations are bound

- `signing_payload()` is the canonical bytes of every field except `signature`. Stable across runs.
- `AgentKeypair::sign_attestation(&mut Attestation)` populates `agent_pubkey` and `signature` together — the public key on the wire matches the key that signed.
- `attestation_leaf_hash(&att) = SHA256(signing_payload || signature)`. Tampering with payload *or* signature changes the leaf, which changes the Merkle root, which fails inclusion verification.
- `verify_inclusion(root, attestation_hash, proof) -> bool` requires only those three inputs — the Phase 3 hard exit criterion.

## Coordination flows

The four examples each demonstrate one coordination shape. The swim lanes below show what's actually shipped, not what was planned.

### Flow 1 — Simple attestation (Worker → Verifier, no Dispatcher)

`examples/simple_attestation.rs` wires Worker and Verifier directly. No Dispatcher, no timeouts, no batcher. Used as the Phase 1 checkpoint and as a smoke test for the signing/verification round-trip.

```mermaid
sequenceDiagram
    participant U as Caller
    participant W as Worker
    participant V as Verifier
    participant T as Tools
    participant FS as attestations.json

    U->>W: run(WorkerTask)
    W->>T: calculate(expression)
    T-->>W: CalculationResult
    W->>W: build Attestation::Completed
    W->>W: sign_attestation
    W-->>U: signed worker_att
    U->>FS: append_attestation(worker_att)

    U->>V: run(WorkerTask, &worker_att)
    V->>T: verify_attestation_struct(&worker_att)
    T-->>V: VerificationResult{sig_valid, hash_valid}
    V->>T: calculate(expression)  [independent re-solve]
    T-->>V: CalculationResult
    V->>V: compare → answer_correct
    V->>V: build Attestation::Verified{...}<br/>references = worker_att.id
    V->>V: sign_attestation
    V-->>U: signed verifier_att
    U->>FS: append_attestation(verifier_att)

    U->>FS: load_attestations() [round-trip check]
    FS-->>U: Vec<Attestation> with valid signatures
```

**Storage:** `data/attestations.json` (JSONL, append-only). Two records per run.

### Flow 2 — Dispatcher (single verifier with timeouts and persistence)

`examples/dispatcher_flow.rs` runs three tasks: a happy path, a worker-timeout path, and a verifier-disagreement path. The Dispatcher owns the lifecycle and writes both the attestation log and the execution log.

```mermaid
sequenceDiagram
    participant U as Caller
    participant D as Dispatcher
    participant W as Worker
    participant V as Verifier
    participant J1 as attestations.json
    participant J2 as executions.json

    U->>D: run(&worker, &verifier, task)
    Note over D: TaskExecution: Pending<br/>→ WorkerRunning

    D->>W: run(task) wrapped in worker_timeout (30s default)
    alt worker times out / errors
        D->>D: TaskExecution.status =<br/>Failed{stage: WorkerRunning, reason}
        D->>J2: append_execution(record)
        D-->>U: Failed TaskExecution
    else worker returns signed attestation
        W-->>D: worker_att
        D->>J1: append_attestation(worker_att)
        Note over D: → WorkerComplete<br/>→ VerifierRunning

        D->>V: run(task, &worker_att) wrapped in verifier_timeout
        alt verifier times out / errors
            D->>D: Failed{stage: VerifierRunning, reason}
        else verifier returns Verified{a, b}
            V-->>D: verifier_att
            D->>J1: append_attestation(verifier_att)
            opt disagreement (a == false || b == false)
                D->>D: log "verifier disagreement" to stderr
            end
            D->>D: status = Complete{<br/>attestation_valid, answer_correct}
        end
        D->>J2: append_execution(TaskExecutionRecord)<br/>id-only references
        D-->>U: completed TaskExecution
    end
```

**Non-obvious behaviour:**

- Verifier disagreement is **logged but not failed**. The Dispatcher transitions to `Complete{false, …}`, not `Failed`. Disagreement is the very signal the system exists to surface.
- Both attestations are persisted to `attestations.json` *before* the execution record is written, so a partial crash leaves attestations intact.
- `TaskExecutionRecord` (the on-disk form) holds attestation **ids** only; `load_execution` rejoins by reading `attestations.json` into a HashMap.

### Flow 3 — Full pipeline (Dispatcher + Batcher → SQLite)

`examples/full_pipeline.rs` runs 12 tasks through a Dispatcher wired to a Batcher with default config (`max_batch_size: 10, max_batch_age_secs: 60, min_batch_size: 1`). At 12 tasks × 2 attestations = 24 submissions, the count trigger fires twice (at 10 and 20) and the shutdown flush seals the remaining 4 — three batches total.

```mermaid
sequenceDiagram
    participant U as Caller
    participant D as Dispatcher
    participant W as Worker
    participant V as Verifier
    participant B as Batcher (bg task)
    participant Buf as buffer (in-memory)
    participant DB as awp.db (SQLite)
    participant J1 as attestations.json

    Note over U: 12 tasks total
    loop per task
        U->>D: run(&worker, &verifier, task)
        D->>W: run(task)
        W-->>D: worker_att
        D->>J1: append_attestation
        D->>V: run(task, &worker_att)
        V-->>D: verifier_att
        D->>J1: append_attestation
        D->>B: submit(worker_att)
        B->>Buf: push
        D->>B: submit(verifier_att)
        B->>Buf: push
    end

    Note over Buf,DB: count trigger: buffer reaches max_batch_size=10
    B->>Buf: drain N attestations
    B->>B: leaves = [attestation_leaf_hash(a) for a in atts]
    B->>B: tree = build_tree(leaves)
    B->>B: proofs = [inclusion_proof(tree, i, ...) for i in 0..N]
    B->>DB: insert_batch_with_proofs(batch, atts, proofs)<br/>(single transaction)

    Note over U,B: at end of run
    U->>B: shutdown()
    Note over Buf: final buffer (e.g. 4 atts)<br/>flushed if len >= min_batch_size
    B->>DB: insert_batch_with_proofs(...)
    B-->>U: final batch id
```

**Verification (asserted by the example):**

```
verify_attestation_inclusion(att_id) → true
  given only:
    - the batch's merkle_root (stored in awp.db)
    - the attestation's stored InclusionProof
    - the attestation itself

tamper(att in awp.db) → leaf_hash changes → verify_attestation_inclusion = false
```

**Storage:** `data/awp.db` is the source of truth for batches, batched attestations, and proofs. `attestations.json` and `executions.json` continue to be appended by the Dispatcher for backwards compatibility — see PAIN_POINTS.md synthesis #4 ("three storage models for one prototype").

### Flow 4 — Parallel verifiers (Worker → N concurrent Verifiers)

`examples/parallel_verifiers.rs` is the Phase 4 stretch task. It runs the same Worker output past three Verifiers concurrently — once with three honest verifiers (unanimous agreement) and once with two honest plus one adversarial (disagreement detected).

```mermaid
sequenceDiagram
    participant U as Caller
    participant PD as ParallelDispatcher
    participant W as Worker
    participant V1 as Verifier 1
    participant V2 as Verifier 2
    participant V3 as Verifier 3
    participant J1 as attestations.json
    participant J2 as executions.json

    U->>PD: run(&worker, &[&v1, &v2, &v3], task)
    PD->>W: run(task) [sequential, with worker_timeout]
    W-->>PD: worker_att
    PD->>J1: append_attestation(worker_att)

    par concurrent fan-out (try_join_all)
        PD->>V1: run(task, &worker_att) + verifier_timeout
        V1-->>PD: v1_att
    and
        PD->>V2: run(task, &worker_att) + verifier_timeout
        V2-->>PD: v2_att
    and
        PD->>V3: run(task, &worker_att) + verifier_timeout
        V3-->>PD: v3_att
    end

    Note over PD: any error / timeout in any verifier<br/>short-circuits → Failed{stage, reason}

    loop per verifier_att
        PD->>J1: append_attestation
    end

    Note over PD: disagreement check:<br/>distinct (att_valid, ans_correct) tuples > 1
    alt all verifiers report same tuple
        PD->>PD: status = Complete{a, b} (consensus)<br/>disagreement = false
    else any pair differs
        PD->>PD: status = Complete{false, false}<br/>disagreement = true<br/>(per-verifier verdicts preserved in vec)
    end

    PD->>J2: append_execution(as_task_execution(record))<br/>first verifier projected as primary
    PD-->>U: ParallelExecution
```

**Non-obvious behaviour:**

- **Disagreement is unanimous-check, not majority vote.** If 2 out of 3 say "valid" and 1 says "invalid", `disagreement = true` and the consensus status is `Complete{false, false}`. Downstream majority logic is recoverable from `verifier_attestations: Vec<Attestation>`.
- **Strict failure policy via `try_join_all`.** Any single verifier failure halts the whole stage. Switching to best-effort (use `join_all`, record per-verifier failures inline) is a one-line change but a real product decision left for the human (DECISIONS.md D4.4).
- **`as_task_execution` projects the N-verifier shape to the Phase 2 single-verifier `TaskExecution`** so the existing `executions.json` reader keeps working. The first verifier becomes the "primary"; the full set is recoverable by reading `attestations.json` by id. PAIN_POINTS.md synthesis #3 flags this as the natural place for a Phase-2-of-AWP coordination-type generalisation.

## Batcher trigger logic

The Batcher is the only piece in the system that runs on a clock as well as on events. The two triggers compose; either fires a flush.

```mermaid
flowchart TD
    Start([Batcher::start]) --> Spawn[spawn background task<br/>+ tokio::time::interval 1s]
    Spawn --> Recv{recv from mpsc}

    Recv -->|Submit| Push[push to buffer<br/>set oldest_at if first]
    Push --> CountCheck{buffer.len &gt;=<br/>max_batch_size?}
    CountCheck -->|yes| Flush
    CountCheck -->|no| Recv

    Recv -->|Flush| Flush
    Recv -->|Shutdown| ShutCheck{buffer.len &gt;=<br/>min_batch_size?}

    ShutCheck -->|yes| FinalFlush[flush final batch]
    ShutCheck -->|no| Discard[discard buffer<br/>too small]
    FinalFlush --> Exit([exit])
    Discard --> Exit

    TimerTick{{1s tick}} -.->|interval| AgeCheck{oldest_at older than<br/>max_batch_age_secs?}
    AgeCheck -->|yes| Flush
    AgeCheck -->|no| Recv

    Flush[/leaves = attestation_leaf_hash for each<br/>tree = build_tree leaves<br/>proofs = inclusion_proof for each<br/>Storage::insert_batch_with_proofs/]
    Flush --> Recv
```

**Defaults:** `max_batch_size: 10, max_batch_age_secs: 60, min_batch_size: 1`. With these, anything submitted gets sealed eventually — the only way to lose an attestation is configuring `min_batch_size > 1` and shutting down with a smaller buffer.

**Latency floor:** the age-trigger polls every 1 second, so a quiet single-attestation submission flushes at `max_batch_age_secs + ~1s`. This is the wakeup cost flagged in PAIN_POINTS.md synthesis #5.

## Storage map

| Artifact | Path | Format | Append-only? | Owner |
|----------|------|--------|--------------|-------|
| Worker / Verifier attestations | `data/attestations.json` | JSONL (1 `Attestation` / line) | yes | Dispatcher, ParallelDispatcher, examples |
| Execution records | `data/executions.json` | JSONL (1 `TaskExecutionRecord` / line, id-only) | yes | Dispatcher, ParallelDispatcher |
| Sealed batches | `data/awp.db` table `batches` | SQLite | INSERT only | Batcher |
| Batched attestations | `data/awp.db` table `attestations` | SQLite, `INSERT OR IGNORE` + later UPDATE for `batch_id` | upsert | Batcher |
| Inclusion proofs | `data/awp.db` table `proofs` (1 row per attestation, JSON BLOB `proof_path`) | SQLite | INSERT only | Batcher |

The Batcher's SQLite write is one transaction per flush — batch row, all attestation upserts, and all proof rows commit together or not at all.

The Phase 1/2 JSONL logs and the Phase 3 SQLite database overlap intentionally during the prototype: the JSONL logs are the durable record of *every* execution, while SQLite is the authoritative store for the *batched* subset. PAIN_POINTS.md synthesis #4 flags this as untenable for production; collapsing to one model is a Phase-2-of-AWP task.

## Tools

Two tools live in `awp-agents/src/tools.rs`:

- **`calculate(expression: &str) -> Result<CalculationResult>`** — recursive-descent parser supporting `+ - * /`, parentheses, and unary minus. Used by both Worker (to produce an answer) and Verifier (to independently re-solve).
- **`verify_attestation`** ships in two forms:
  - `verify_attestation(json: &str) -> Result<VerificationResult>` — JSON-string surface for an LLM-driven tool runtime
  - `verify_attestation_struct(att: &Attestation) -> VerificationResult` — typed in-process surface used by the Verifier

The dual surface is PAIN_POINTS.md synthesis #2 — currently ~5 lines of glue, but every typed value would need a serialised twin if every tool also exposed a JSON face. The recommendation in DECISIONS.md (Option C — thin custom layer on Rig) collapses this duplication by owning the agent loop and using JSON only at the LLM boundary.

## What is *not* implemented

These appear in the planning docs but are **explicitly deferred** and not present in the current code:

- **AutoAgents framework integration.** Worker and Verifier are plain async types; the framework named "primary" in the plan has zero lines of code in the implementation (DECISIONS.md D1.1, PAIN_POINTS.md synthesis #1).
- **On-chain anchoring.** `Batch.anchor_tx` and `Batch.anchor_chain` exist but are always `None` / `NULL`. No chain client.
- **Persistent agent identity.** `AgentKeypair::generate()` is called per Worker/Verifier construction. No on-disk identity, no key rotation, no operator linkage.
- **HTTP / external API.** The prototype is a CLI / library only.
- **LuaI execution layer (Appendix A of the plan).** No `trace_hash`, no `zk_proof`, no Lua runtime.

## See also

- [`PHASE1_REVIEW.md`](PHASE1_REVIEW.md) — synthesised executive summary of Phase 1's outcome
- [`DECISIONS.md`](DECISIONS.md) — design decisions log + framework recommendation
- [`PAIN_POINTS.md`](PAIN_POINTS.md) — friction log + Phase 4 synthesis
- [`../planning/awp-prototype-plan.md`](../planning/awp-prototype-plan.md) — original 8-week plan + Phase 1 Postmortem
