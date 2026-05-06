//! Shared tools that agents call during task execution.
//!
//! Phase 1 ships two tools:
//!
//! - [`calculate`] — a tiny arithmetic evaluator the Worker uses to do
//!   the task and the Verifier uses for its independent re-solve.
//! - [`verify_attestation`] — the cryptographic check the Verifier
//!   performs on the Worker's attestation before reasoning about
//!   correctness, exactly as specified in
//!   `planning/awp-prototype-plan.md` → "Verifier Tool: verify_attestation".
//!
//! Both tools are plain functions today. Phase 4's evaluation will revisit
//! whether to wrap them with the AutoAgents `#[tool]` macro once the
//! framework decision is made (see `docs/PAIN_POINTS.md`).

use awp_core::{sha256, verify_attestation_signature, Attestation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result of running [`verify_attestation`] on a Worker attestation.
///
/// Field shape matches the plan's "Verifier Tool" subsection:
/// `signature_valid`, `hash_valid`, `overall_valid`, `agent_pubkey`
/// (hex-encoded), and `timestamp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub signature_valid: bool,
    pub hash_valid: bool,
    pub overall_valid: bool,
    pub agent_pubkey: String,
    pub timestamp: i64,
}

/// Verify an attestation's cryptographic signature and hash integrity.
///
/// Accepts the JSON encoding of the attestation (matching the plan's
/// `verify_attestation(attestation_json: String)` signature) so the tool
/// can be called by an agent runtime that passes opaque strings between
/// tool and LLM.
pub fn verify_attestation(attestation_json: &str) -> Result<VerificationResult, ToolError> {
    let attestation: Attestation = serde_json::from_str(attestation_json)?;
    Ok(verify_attestation_struct(&attestation))
}

/// Same as [`verify_attestation`] but takes the parsed struct directly —
/// preferred when an in-process caller already has the [`Attestation`].
pub fn verify_attestation_struct(attestation: &Attestation) -> VerificationResult {
    let signature_valid = verify_attestation_signature(attestation);
    let computed_output_hash = sha256(attestation.output.as_bytes());
    let hash_valid = computed_output_hash == attestation.output_hash;
    VerificationResult {
        signature_valid,
        hash_valid,
        overall_valid: signature_valid && hash_valid,
        agent_pubkey: hex::encode(attestation.agent_pubkey),
        timestamp: attestation.timestamp,
    }
}

/// Result of running [`calculate`] on an arithmetic expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalculationResult {
    pub expression: String,
    pub result: f64,
}

/// Evaluate a small arithmetic expression. Supports `+`, `-`, `*`, `/`
/// over decimal numbers with operator precedence and parentheses — enough
/// for the Phase 1 example task ("140 * 0.6") and a reasonable surface for
/// the Verifier to independently re-solve.
pub fn calculate(expression: &str) -> Result<CalculationResult, ToolError> {
    let value = eval::evaluate(expression)?;
    Ok(CalculationResult {
        expression: expression.to_string(),
        result: value,
    })
}

/// Errors returned by the shared tools.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid attestation json: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("calculation failed: {0}")]
    Calc(String),
}

mod eval {
    //! Tiny recursive-descent arithmetic evaluator.
    //!
    //! Grammar:
    //!   expr   := term (('+' | '-') term)*
    //!   term   := factor (('*' | '/') factor)*
    //!   factor := number | '(' expr ')' | '-' factor

    use super::ToolError;

    pub fn evaluate(input: &str) -> Result<f64, ToolError> {
        let mut p = Parser {
            bytes: input.as_bytes(),
            pos: 0,
        };
        let v = p.expr()?;
        p.skip_ws();
        if p.pos != p.bytes.len() {
            return Err(ToolError::Calc(format!(
                "unexpected trailing input at byte {}",
                p.pos
            )));
        }
        Ok(v)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        pos: usize,
    }

    impl Parser<'_> {
        fn skip_ws(&mut self) {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
        }

        fn peek(&mut self) -> Option<u8> {
            self.skip_ws();
            self.bytes.get(self.pos).copied()
        }

        fn expr(&mut self) -> Result<f64, ToolError> {
            let mut acc = self.term()?;
            loop {
                match self.peek() {
                    Some(b'+') => {
                        self.pos += 1;
                        acc += self.term()?;
                    }
                    Some(b'-') => {
                        self.pos += 1;
                        acc -= self.term()?;
                    }
                    _ => break,
                }
            }
            Ok(acc)
        }

        fn term(&mut self) -> Result<f64, ToolError> {
            let mut acc = self.factor()?;
            loop {
                match self.peek() {
                    Some(b'*') => {
                        self.pos += 1;
                        acc *= self.factor()?;
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        let rhs = self.factor()?;
                        if rhs == 0.0 {
                            return Err(ToolError::Calc("division by zero".into()));
                        }
                        acc /= rhs;
                    }
                    _ => break,
                }
            }
            Ok(acc)
        }

        fn factor(&mut self) -> Result<f64, ToolError> {
            match self.peek() {
                Some(b'-') => {
                    self.pos += 1;
                    Ok(-self.factor()?)
                }
                Some(b'(') => {
                    self.pos += 1;
                    let v = self.expr()?;
                    match self.peek() {
                        Some(b')') => {
                            self.pos += 1;
                            Ok(v)
                        }
                        _ => Err(ToolError::Calc("missing closing paren".into())),
                    }
                }
                Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
                Some(c) => Err(ToolError::Calc(format!(
                    "unexpected character {:?} at byte {}",
                    c as char, self.pos
                ))),
                None => Err(ToolError::Calc("unexpected end of input".into())),
            }
        }

        fn number(&mut self) -> Result<f64, ToolError> {
            let start = self.pos;
            while self.pos < self.bytes.len() {
                let b = self.bytes[self.pos];
                if b.is_ascii_digit() || b == b'.' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let s = std::str::from_utf8(&self.bytes[start..self.pos])
                .map_err(|_| ToolError::Calc("non-utf8 number".into()))?;
            s.parse::<f64>()
                .map_err(|e| ToolError::Calc(format!("bad number {s:?}: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awp_core::{AgentKeypair, Attestation, AttestationStatus};

    fn make_completed(kp: &AgentKeypair, output: &str) -> Attestation {
        let task_hash = sha256(b"task");
        let mut a = Attestation::new(
            "worker-1",
            task_hash,
            output,
            AttestationStatus::Completed,
            None,
            1_700_000_000,
        );
        kp.sign_attestation(&mut a);
        a
    }

    #[test]
    fn calculate_basic_arithmetic() {
        let r = calculate("140 * 0.6").unwrap();
        assert!((r.result - 84.0).abs() < 1e-9);
        assert_eq!(r.expression, "140 * 0.6");
    }

    #[test]
    fn calculate_respects_precedence() {
        let r = calculate("2 + 3 * 4").unwrap();
        assert!((r.result - 14.0).abs() < 1e-9);
    }

    #[test]
    fn calculate_parens_and_unary() {
        let r = calculate("-(2 + 3) * 4").unwrap();
        assert!((r.result + 20.0).abs() < 1e-9);
    }

    #[test]
    fn calculate_rejects_bad_input() {
        assert!(calculate("2 +").is_err());
        assert!(calculate("(1 + 2").is_err());
        assert!(calculate("1 / 0").is_err());
        assert!(calculate("abc").is_err());
    }

    #[test]
    fn verify_attestation_accepts_valid_json() {
        let kp = AgentKeypair::generate();
        let a = make_completed(&kp, "84");
        let json = serde_json::to_string(&a).unwrap();
        let r = verify_attestation(&json).unwrap();
        assert!(r.signature_valid);
        assert!(r.hash_valid);
        assert!(r.overall_valid);
        assert_eq!(r.agent_pubkey, hex::encode(kp.public_bytes()));
        assert_eq!(r.timestamp, a.timestamp);
    }

    #[test]
    fn verify_attestation_rejects_tampered_output() {
        let kp = AgentKeypair::generate();
        let mut a = make_completed(&kp, "84");
        a.output = "evil".into();
        let json = serde_json::to_string(&a).unwrap();
        let r = verify_attestation(&json).unwrap();
        assert!(!r.signature_valid);
        assert!(!r.hash_valid, "output_hash no longer matches output");
        assert!(!r.overall_valid);
    }

    #[test]
    fn verify_attestation_rejects_tampered_signature() {
        let kp = AgentKeypair::generate();
        let mut a = make_completed(&kp, "84");
        a.signature[0] ^= 0xff;
        let json = serde_json::to_string(&a).unwrap();
        let r = verify_attestation(&json).unwrap();
        assert!(!r.signature_valid);
        assert!(r.hash_valid);
        assert!(!r.overall_valid);
    }

    #[test]
    fn verify_attestation_struct_matches_json_path() {
        let kp = AgentKeypair::generate();
        let a = make_completed(&kp, "84");
        let json = serde_json::to_string(&a).unwrap();
        let from_json = verify_attestation(&json).unwrap();
        let from_struct = verify_attestation_struct(&a);
        assert_eq!(from_json, from_struct);
    }

    #[test]
    fn verify_attestation_rejects_invalid_json() {
        assert!(matches!(
            verify_attestation("{not json"),
            Err(ToolError::InvalidJson(_))
        ));
    }
}
