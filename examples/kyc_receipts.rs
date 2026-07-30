//! GTM Phase 1 — KYC Receipts demo.
//!
//! Replaces the arithmetic task that drove the Phase 1 examples with a
//! deterministic KYC decision rule. The flow is the same as
//! `simple_attestation.rs` — Worker decides, Verifier independently
//! re-decides — but the decision is over a customer transaction (per the
//! Persona A user journey in `docs/USER_JOURNEYS.md`) rather than a string
//! expression.
//!
//! Three scenarios are printed in order:
//!
//! 1. **Approve** — small domestic card transaction. Worker approves;
//!    Verifier independently approves. Receipt is audit-ready.
//! 2. **Flag**    — large amount. Worker flags with reason "amount exceeds
//!    threshold"; Verifier agrees. Receipt is audit-ready.
//! 3. **Tampered** — Worker flags, signs, then we mutate the in-memory
//!    attestation's `output` field after signing. The Verifier's crypto
//!    check fails (`attestation_valid: false`) and the demo prints a
//!    customer-resonant rejection.
//!
//! The example appends to `data/attestations.json` and `data/executions.json`
//! so the audit viewer (Step 1 of GTM Phase 1) has something legible to
//! render.

use std::path::PathBuf;

use attestly_agents::{decision_output, KycVerifier, KycVerifierAgent, KycWorker, KycWorkerAgent};
use attestly_core::{
    append_attestation, append_execution, AgentIdentity, Attestation, AttestationStatus,
    ExecutionStatus, FileIdentityStore, KycDecision, KycRequest, TaskExecution,
};

const ATTESTATIONS_PATH: &str = "data/attestations.json";
const EXECUTIONS_PATH: &str = "data/executions.json";
const IDENTITIES_DIR: &str = "data/identities";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let attestations_path = PathBuf::from(ATTESTATIONS_PATH);
    let executions_path = PathBuf::from(EXECUTIONS_PATH);

    // Persistent identities: load existing keypairs from disk, or generate
    // and save fresh ones if this is the first run. Re-running the demo
    // produces attestations from the same `agent_pubkey` for both agents
    // until the underlying JSON files are removed.
    let store = FileIdentityStore::new(IDENTITIES_DIR);
    let worker_identity = AgentIdentity::load_or_create(&store, "agent-kyc-01")?;
    let verifier_identity = AgentIdentity::load_or_create(&store, "agent-kyc-02")?;
    let worker = KycWorker::with_identity(worker_identity);
    let verifier = KycVerifier::with_identity(verifier_identity);

    // -- Scenario 1: Approve -------------------------------------------------
    let approve_req = KycRequest::new("4711", 8_999, "US", "card");
    run_clean_scenario(
        "Customer #4711",
        approve_req,
        &worker,
        &verifier,
        &attestations_path,
        &executions_path,
    )
    .await?;

    // -- Scenario 2: Flag (large amount) ------------------------------------
    let flag_req = KycRequest::new("8842", 5_000_000, "US", "card");
    run_clean_scenario(
        "Customer #8842",
        flag_req,
        &worker,
        &verifier,
        &attestations_path,
        &executions_path,
    )
    .await?;

    // -- Scenario 3: Tampered ------------------------------------------------
    let tampered_req = KycRequest::new("9999", 2_500_000, "US", "card");
    run_tampered_scenario(
        "Customer #9999",
        tampered_req,
        &worker,
        &verifier,
        &attestations_path,
        &executions_path,
    )
    .await?;

    println!();
    println!(
        "Attestations appended to {} ; executions appended to {} .",
        attestations_path.display(),
        executions_path.display(),
    );
    println!("Open tools/audit-viewer/index.html and drag those files in to inspect.");

    Ok(())
}

/// Run a single Worker → Verifier round, persist both attestations + the
/// joining execution record, and print a customer-resonant summary. Used
/// for the Approve and Flag scenarios.
async fn run_clean_scenario(
    customer_label: &str,
    request: KycRequest,
    worker: &KycWorker,
    verifier: &KycVerifier,
    attestations_path: &std::path::Path,
    executions_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = chrono::Utc::now().timestamp();
    let mut execution = TaskExecution::new(execution_label(&request), started_at);

    let worker_att = worker.run(&request).await?;
    append_attestation(attestations_path, &worker_att)?;
    execution.worker_attestation = Some(worker_att.clone());

    let verifier_att = verifier.run(&request, &worker_att).await?;
    append_attestation(attestations_path, &verifier_att)?;
    execution.verifier_attestation = Some(verifier_att.clone());

    let (attestation_valid, answer_correct) = unwrap_verified(&verifier_att);
    execution.status = ExecutionStatus::Complete {
        attestation_valid,
        answer_correct,
    };
    execution.completed_at = Some(chrono::Utc::now().timestamp());
    append_execution(executions_path, &execution)?;

    print_clean_summary(
        customer_label,
        &worker_att,
        &verifier_att,
        attestation_valid,
        answer_correct,
        attestations_path,
    );

    Ok(())
}

/// Run a Worker → Verifier round with deliberate tamper after signing. The
/// Verifier should report `attestation_valid: false` and the demo prints a
/// rejection message explaining why.
async fn run_tampered_scenario(
    customer_label: &str,
    request: KycRequest,
    worker: &KycWorker,
    verifier: &KycVerifier,
    attestations_path: &std::path::Path,
    executions_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = chrono::Utc::now().timestamp();
    let mut execution = TaskExecution::new(execution_label(&request), started_at);

    // The Worker correctly emits Flag for this request (amount > threshold).
    let mut worker_att = worker.run(&request).await?;
    let original_decision: KycDecision = serde_json::from_str(&worker_att.output)?;

    // ---- The tamper -------------------------------------------------------
    // We mutate the in-memory attestation's `output` field *after* signing.
    // The signature still covers the original output_hash, so the Verifier's
    // crypto check will fail. The persisted record on disk is the tampered
    // form, so the audit viewer renders the same broken state.
    worker_att.output = decision_output(&KycDecision::Approve);
    // ----------------------------------------------------------------------

    append_attestation(attestations_path, &worker_att)?;
    execution.worker_attestation = Some(worker_att.clone());

    let verifier_att = verifier.run(&request, &worker_att).await?;
    append_attestation(attestations_path, &verifier_att)?;
    execution.verifier_attestation = Some(verifier_att.clone());

    let (attestation_valid, answer_correct) = unwrap_verified(&verifier_att);
    execution.status = ExecutionStatus::Complete {
        attestation_valid,
        answer_correct,
    };
    execution.completed_at = Some(chrono::Utc::now().timestamp());
    append_execution(executions_path, &execution)?;

    print_tampered_summary(
        customer_label,
        &original_decision,
        attestation_valid,
        answer_correct,
    );

    Ok(())
}

/// Brief input summary used as `task_input` on the [`TaskExecution`].
fn execution_label(req: &KycRequest) -> String {
    format!(
        "kyc:cust={} amount={} jurisdiction={} type={}",
        req.customer_id, req.amount_cents, req.jurisdiction, req.transaction_type
    )
}

/// Pull `(attestation_valid, answer_correct)` out of a Verifier attestation.
/// Falls back to `(false, false)` if the status isn't `Verified`; in this
/// demo the Verifier always emits `Verified`, so the fallback is defensive.
fn unwrap_verified(att: &Attestation) -> (bool, bool) {
    match att.status {
        AttestationStatus::Verified {
            attestation_valid,
            answer_correct,
        } => (attestation_valid, answer_correct),
        _ => (false, false),
    }
}

fn print_clean_summary(
    customer_label: &str,
    worker_att: &Attestation,
    verifier_att: &Attestation,
    attestation_valid: bool,
    answer_correct: bool,
    attestations_path: &std::path::Path,
) {
    let decision: KycDecision = serde_json::from_str(&worker_att.output)
        .expect("worker output is a serialized KycDecision in clean scenarios");

    println!();
    println!("{customer_label} — Decision: {}", decision.tag());
    if let Some(reasons) = decision_reasons(&decision) {
        println!("  Reasons:    {}", reasons.join(", "));
    }
    println!("  Worker:     {}  signature \u{2713}", worker_att.agent_id);
    println!(
        "  Verifier:   {}  signature {} verdict {}",
        verifier_att.agent_id,
        if attestation_valid {
            "\u{2713}"
        } else {
            "\u{2717}"
        },
        if answer_correct {
            "\u{2713}"
        } else {
            "\u{2717}"
        },
    );
    println!(
        "  Receipt:    {} (id: {})",
        attestations_path.display(),
        worker_att.id,
    );
    println!(
        "  Audit-ready: {}",
        if attestation_valid && answer_correct {
            "yes"
        } else {
            "no"
        }
    );
}

fn print_tampered_summary(
    customer_label: &str,
    original_decision: &KycDecision,
    attestation_valid: bool,
    answer_correct: bool,
) {
    println!();
    println!("{customer_label} — Decision: {}", original_decision.tag());
    if let Some(reasons) = decision_reasons(original_decision) {
        println!("  Reasons:    {}", reasons.join(", "));
    }
    println!("  Worker:     agent-kyc-01  signature \u{2713} (at sign time)");
    println!(
        "  Verifier:   agent-kyc-02  signature {} \u{2014} receipt rejected",
        if attestation_valid {
            "\u{2713}"
        } else {
            "\u{2717}"
        },
    );
    println!("  Reason:     payload was modified after signing; receipt is not audit-ready.");
    println!(
        "  Verdict:    {}",
        if answer_correct {
            "\u{2713}"
        } else {
            "\u{2717}"
        }
    );
}

fn decision_reasons(decision: &KycDecision) -> Option<&[String]> {
    match decision {
        KycDecision::Approve => None,
        KycDecision::Flag { reasons } | KycDecision::Reject { reasons } => Some(reasons),
    }
}
