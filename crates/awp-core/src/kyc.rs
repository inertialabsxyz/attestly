//! KYC (Know Your Customer) request and decision types for the
//! `kyc_receipts` demo.
//!
//! These live in `awp-core` so the demo's domain types are framework-agnostic
//! and can be parsed by any consumer (CLI, audit viewer, future SDKs) without
//! pulling in `awp-agents`. The decision rule itself lives in
//! `awp-agents::tools::kyc_decide` since that's where the Worker and Verifier
//! invoke it.
//!
//! ## Disclaimer
//!
//! Demo data only. The jurisdiction codes (`XX`, `YY`) are deliberate
//! ISO-3166-style placeholders chosen to avoid implying any real geopolitical
//! claim. The thresholds and rules are illustrative — they are *not* a
//! production KYC policy and must not be used as one.

use serde::{Deserialize, Serialize};

/// A KYC review request. Mirrors a minimal slice of the inputs a real-world
/// risk-rule engine would consume; see the module disclaimer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KycRequest {
    pub customer_id: String,
    pub amount_cents: u64,
    pub jurisdiction: String,
    pub transaction_type: String,
}

impl KycRequest {
    pub fn new(
        customer_id: impl Into<String>,
        amount_cents: u64,
        jurisdiction: impl Into<String>,
        transaction_type: impl Into<String>,
    ) -> Self {
        Self {
            customer_id: customer_id.into(),
            amount_cents,
            jurisdiction: jurisdiction.into(),
            transaction_type: transaction_type.into(),
        }
    }

    /// Canonical bytes for the request — what `task_hash` covers.
    ///
    /// `serde_json` preserves struct field order, giving us a deterministic
    /// encoding without an extra dependency. This mirrors
    /// [`crate::attestation::Attestation::signing_payload`]'s approach.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("KycRequest serialization is infallible")
    }
}

/// The outcome of running [`crate::kyc::KycRequest`] through the decision
/// rule. `Flag` and `Reject` carry structured reasons so receipts surface
/// *why* the rule fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KycDecision {
    Approve,
    Flag { reasons: Vec<String> },
    Reject { reasons: Vec<String> },
}

impl KycDecision {
    /// Short tag for stdout — matches the customer-resonant strings in
    /// `examples/kyc_receipts.rs`.
    pub fn tag(&self) -> &'static str {
        match self {
            KycDecision::Approve => "APPROVE",
            KycDecision::Flag { .. } => "FLAG",
            KycDecision::Reject { .. } => "REJECT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_bytes_round_trip() {
        let req = KycRequest::new("cust-1", 500, "US", "card");
        let bytes = req.canonical_bytes();
        let back: KycRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn canonical_bytes_are_deterministic() {
        let req = KycRequest::new("cust-1", 500, "US", "card");
        assert_eq!(req.canonical_bytes(), req.canonical_bytes());
    }

    #[test]
    fn canonical_bytes_change_when_field_changes() {
        let a = KycRequest::new("cust-1", 500, "US", "card");
        let b = KycRequest::new("cust-1", 501, "US", "card");
        assert_ne!(a.canonical_bytes(), b.canonical_bytes());
    }

    #[test]
    fn decision_round_trips_through_serde() {
        let d = KycDecision::Flag {
            reasons: vec!["high-risk jurisdiction".into()],
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: KycDecision = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn decision_tags() {
        assert_eq!(KycDecision::Approve.tag(), "APPROVE");
        assert_eq!(
            KycDecision::Flag {
                reasons: vec!["x".into()]
            }
            .tag(),
            "FLAG"
        );
        assert_eq!(
            KycDecision::Reject {
                reasons: vec!["x".into()]
            }
            .tag(),
            "REJECT"
        );
    }
}
