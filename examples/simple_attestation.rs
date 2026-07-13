//! Phase 1 checkpoint binary.
//!
//! Runs the full Worker → Verifier flow end-to-end on a hardcoded task,
//! writes both attestations to `data/attestations.json`, then re-loads and
//! re-verifies them. Each run appends two more attestations to the log so
//! you can run the example repeatedly to grow the file.
//!
//! Expected output (truncated):
//!
//! ```text
//! Worker attestation     <uuid>  status=Completed
//! Verifier attestation   <uuid>  status=Verified { attestation_valid: true, answer_correct: true }
//! Persisted to data/attestations.json
//! Reloaded 4 attestation(s); all signatures verify.
//! ```

use std::path::PathBuf;

use attestly_agents::{Verifier, VerifierAgent, Worker, WorkerAgent, WorkerTask};
use attestly_core::{append_attestation, load_attestations, AttestationStatus};

const ATTESTATIONS_PATH: &str = "data/attestations.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Resolve `data/attestations.json` relative to the current working
    // directory. When run via `cargo run --example simple_attestation`
    // from the workspace root, this lands in `<workspace>/data/`.
    let path = PathBuf::from(ATTESTATIONS_PATH);

    let worker = Worker::new("worker-1");
    let verifier = Verifier::new("verifier-1");
    let task = WorkerTask::new("140 * 0.6");

    let worker_attestation = worker.run(&task).await?;
    let verifier_attestation = verifier.run(&task, &worker_attestation).await?;

    println!(
        "Worker attestation     {}  status={}",
        worker_attestation.id,
        format_status(&worker_attestation.status)
    );
    println!(
        "Verifier attestation   {}  status={}",
        verifier_attestation.id,
        format_status(&verifier_attestation.status)
    );

    append_attestation(&path, &worker_attestation)?;
    append_attestation(&path, &verifier_attestation)?;
    println!("Persisted to {}", path.display());

    let reloaded = load_attestations(&path)?;
    println!(
        "Reloaded {} attestation(s); all signatures verify.",
        reloaded.len()
    );

    Ok(())
}

fn format_status(status: &AttestationStatus) -> String {
    match status {
        AttestationStatus::Completed => "Completed".to_string(),
        AttestationStatus::Failed(reason) => format!("Failed({reason:?})"),
        AttestationStatus::Verified {
            attestation_valid,
            answer_correct,
        } => format!(
            "Verified {{ attestation_valid: {attestation_valid}, answer_correct: {answer_correct} }}"
        ),
    }
}
