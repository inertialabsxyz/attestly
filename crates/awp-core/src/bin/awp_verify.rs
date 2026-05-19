//! Cross-language verifier binary for AWP attestations.
//!
//! Reads a JSON-encoded [`Attestation`] from stdin and exits 0 if the
//! signature verifies, 1 if it does not, 2 on input errors. Used by the
//! `crates/awp-python/tests/cross_language.py` self-test to confirm that
//! a Python-produced attestation verifies in Rust without any code path
//! shared between the two sides.
//!
//! Usage:
//!
//! ```sh
//! python sign.py | awp-verify        # verify
//! awp-verify --print-canonical < att.json   # emit signing_payload bytes
//! ```

use std::io::{self, Read, Write};
use std::process::ExitCode;

use awp_core::attestation::Attestation;
use awp_core::signing::verify_attestation_signature;

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    let print_canonical = matches!(argv.next().as_deref(), Some("--print-canonical"));

    let mut buf = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut buf) {
        eprintln!("awp-verify: failed to read stdin: {e}");
        return ExitCode::from(2);
    }

    let att: Attestation = match serde_json::from_str(&buf) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("awp-verify: failed to parse attestation JSON: {e}");
            return ExitCode::from(2);
        }
    };

    if print_canonical {
        let bytes = att.signing_payload();
        if io::stdout().write_all(&bytes).is_err() {
            return ExitCode::from(2);
        }
        return ExitCode::SUCCESS;
    }

    if verify_attestation_signature(&att) {
        println!("ok");
        ExitCode::SUCCESS
    } else {
        eprintln!("awp-verify: signature did not verify");
        ExitCode::from(1)
    }
}
