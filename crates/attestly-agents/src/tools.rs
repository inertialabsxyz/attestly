//! Shared tools that agents call during task execution.
//!
//! Phase 1 ships two tools:
//!
//! - [`calculate`] — a tiny arithmetic evaluator the Worker uses to do
//!   the task and the Verifier uses for its independent re-solve.
//! - [`verify_attestation`] — the cryptographic check the Verifier
//!   performs on the Worker's attestation before reasoning about
//!   correctness, exactly as specified in
//!   `planning/attestly-prototype-plan.md` → "Verifier Tool: verify_attestation".
//!
//! Both tools are plain functions today. Phase 4's evaluation will revisit
//! whether to wrap them with the AutoAgents `#[tool]` macro once the
//! framework decision is made (see `docs/PAIN_POINTS.md`).

use attestly_core::{sha256, verify_attestation_signature, Attestation, KycDecision, KycRequest};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Jurisdiction codes treated as high-risk by the demo rule.
///
/// Deliberate ISO-3166-style placeholders to avoid implying any real
/// geopolitical claim. Demo-only; not a production sanctions or risk list.
pub const HIGH_RISK_JURISDICTIONS: &[&str] = &["XX", "YY"];

/// Amount threshold (in cents) above which a transaction is flagged for
/// review. $10,000 USD-equivalent — illustrative only, not derived from any
/// real regulatory threshold.
pub const FLAG_AMOUNT_CENTS: u64 = 1_000_000;

/// Amount threshold (in cents) above which a wire transfer is rejected and
/// routed to manual review. $100,000 USD-equivalent — illustrative only.
pub const WIRE_REJECT_AMOUNT_CENTS: u64 = 10_000_000;

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

/// Run the demo KYC decision rule over a [`KycRequest`].
///
/// **Deterministic and pure.** Both Worker and Verifier call this independently
/// — the rule must produce identical output across runs and processes for the
/// receipts demo to make sense. See the `kyc` module rustdoc for the demo
/// disclaimer.
///
/// Rule (evaluated top-to-bottom; the first matching branch returns):
///
/// 1. `transaction_type == "wire"` and `amount_cents > WIRE_REJECT_AMOUNT_CENTS`
///    → **Reject** with reason "high-value wire requires manual review".
/// 2. `jurisdiction` is in [`HIGH_RISK_JURISDICTIONS`] → **Flag** with reason
///    "high-risk jurisdiction".
/// 3. `amount_cents > FLAG_AMOUNT_CENTS` → **Flag** with reason "amount
///    exceeds threshold".
/// 4. Otherwise → **Approve**.
///
/// A request can match multiple Flag conditions (e.g. high-risk jurisdiction
/// *and* large amount). All matching reasons are collected so the receipt
/// captures the full picture.
pub fn kyc_decide(request: &KycRequest) -> KycDecision {
    if request.transaction_type == "wire" && request.amount_cents > WIRE_REJECT_AMOUNT_CENTS {
        return KycDecision::Reject {
            reasons: vec!["high-value wire requires manual review".to_string()],
        };
    }

    let mut flag_reasons = Vec::new();
    if HIGH_RISK_JURISDICTIONS
        .iter()
        .any(|j| *j == request.jurisdiction)
    {
        flag_reasons.push("high-risk jurisdiction".to_string());
    }
    if request.amount_cents > FLAG_AMOUNT_CENTS {
        flag_reasons.push("amount exceeds threshold".to_string());
    }

    if flag_reasons.is_empty() {
        KycDecision::Approve
    } else {
        KycDecision::Flag {
            reasons: flag_reasons,
        }
    }
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
    use attestly_core::{AgentKeypair, Attestation, AttestationStatus};

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

    // ---- kyc_decide ------------------------------------------------------

    #[test]
    fn kyc_approve_small_domestic_card() {
        let req = KycRequest::new("cust-100", 499, "US", "card");
        assert_eq!(kyc_decide(&req), KycDecision::Approve);
    }

    #[test]
    fn kyc_flag_high_risk_jurisdiction() {
        let req = KycRequest::new("cust-200", 500, "XX", "card");
        assert_eq!(
            kyc_decide(&req),
            KycDecision::Flag {
                reasons: vec!["high-risk jurisdiction".into()]
            }
        );
    }

    #[test]
    fn kyc_flag_amount_above_threshold() {
        let req = KycRequest::new("cust-300", FLAG_AMOUNT_CENTS + 1, "US", "card");
        assert_eq!(
            kyc_decide(&req),
            KycDecision::Flag {
                reasons: vec!["amount exceeds threshold".into()]
            }
        );
    }

    #[test]
    fn kyc_flag_collects_multiple_reasons() {
        let req = KycRequest::new("cust-301", FLAG_AMOUNT_CENTS + 1, "YY", "card");
        match kyc_decide(&req) {
            KycDecision::Flag { reasons } => {
                assert_eq!(
                    reasons,
                    vec![
                        "high-risk jurisdiction".to_string(),
                        "amount exceeds threshold".to_string()
                    ]
                );
            }
            other => panic!("expected Flag, got {other:?}"),
        }
    }

    #[test]
    fn kyc_reject_high_value_wire() {
        let req = KycRequest::new("cust-400", WIRE_REJECT_AMOUNT_CENTS + 1, "US", "wire");
        assert_eq!(
            kyc_decide(&req),
            KycDecision::Reject {
                reasons: vec!["high-value wire requires manual review".into()]
            }
        );
    }

    #[test]
    fn kyc_wire_at_or_below_reject_threshold_only_flags() {
        // Right at the reject threshold: not strictly greater, so Reject does
        // not fire. The amount is still over the flag threshold so we Flag.
        let req = KycRequest::new("cust-401", WIRE_REJECT_AMOUNT_CENTS, "US", "wire");
        assert_eq!(
            kyc_decide(&req),
            KycDecision::Flag {
                reasons: vec!["amount exceeds threshold".into()]
            }
        );
    }

    #[test]
    fn kyc_amount_at_flag_threshold_is_approved() {
        // Boundary: equal to threshold is *not* strictly greater, so Approve.
        let req = KycRequest::new("cust-500", FLAG_AMOUNT_CENTS, "US", "card");
        assert_eq!(kyc_decide(&req), KycDecision::Approve);
    }

    #[test]
    fn kyc_reject_takes_precedence_over_flag_branches() {
        // A request that would match both Reject (high-value wire) and Flag
        // (high-risk jurisdiction) — Reject wins because it evaluates first.
        let req = KycRequest::new("cust-600", WIRE_REJECT_AMOUNT_CENTS + 1, "XX", "wire");
        assert_eq!(
            kyc_decide(&req),
            KycDecision::Reject {
                reasons: vec!["high-value wire requires manual review".into()]
            }
        );
    }

    #[test]
    fn kyc_decide_is_deterministic() {
        let req = KycRequest::new("cust-700", FLAG_AMOUNT_CENTS + 1, "YY", "card");
        let first = kyc_decide(&req);
        let second = kyc_decide(&req);
        assert_eq!(first, second);
    }
}
