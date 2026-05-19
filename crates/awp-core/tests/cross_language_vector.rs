//! Deterministic cross-language signing vector.
//!
//! This test pins the exact bytes and signature that the AWP canonical
//! encoding produces for a fixed payload, identity seed, attestation UUID,
//! and timestamp. The Python-side test at
//! `crates/awp-python/tests/cross_language.py` asserts the **same** byte
//! and signature values — so a divergence in either canonical encoding
//! immediately fails on both sides.
//!
//! Update protocol: if a canonical-encoding change is intentional, both
//! the constants here AND the constants in the Python test must be
//! updated together, in the same commit.

use awp_core::attestation::{Attestation, AttestationStatus};
use awp_core::signing::{sha256, verify_attestation_signature, AgentKeypair};
use uuid::Uuid;

/// Hex-encoded ed25519 secret used for the cross-language signing vector.
/// 32 bytes of 0x07. Deterministic — never use this seed for anything but
/// tests.
const VECTOR_SECRET_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";
const VECTOR_AGENT_ID: &str = "agent-vector";
const VECTOR_ATTESTATION_ID: &str = "11111111-2222-4333-8444-555555555555";
const VECTOR_TIMESTAMP: i64 = 1_700_000_000;

/// The payload, presented in a deliberately non-alphabetical insertion
/// order. The canonical encoding must reorder this to alphabetical keys.
const VECTOR_PAYLOAD_INSERTION_ORDER: &str = r#"{"z_field":"last alphabetically","amount_cents":50000,"action":"approve","nested":{"y":true,"a":[1,2,3]}}"#;

/// The canonical bytes that `awp.canonical_payload_bytes(payload)` must
/// produce. Keys sorted alphabetically by serde_json's default
/// `BTreeMap` ordering.
const EXPECTED_CANONICAL_PAYLOAD: &[u8] = br#"{"action":"approve","amount_cents":50000,"nested":{"a":[1,2,3],"y":true},"z_field":"last alphabetically"}"#;

fn build_vector_attestation() -> Attestation {
    let mut secret = [0u8; 32];
    hex::decode_to_slice(VECTOR_SECRET_HEX, &mut secret).unwrap();
    let kp = AgentKeypair::from_secret_bytes(&secret, None).unwrap();

    let task_hash = sha256(EXPECTED_CANONICAL_PAYLOAD);
    let output = std::str::from_utf8(EXPECTED_CANONICAL_PAYLOAD)
        .unwrap()
        .to_string();

    let mut att = Attestation::new(
        VECTOR_AGENT_ID,
        task_hash,
        output,
        AttestationStatus::Completed,
        None,
        VECTOR_TIMESTAMP,
    );
    att.id = Uuid::parse_str(VECTOR_ATTESTATION_ID).unwrap();
    kp.sign_attestation(&mut att);
    att
}

#[test]
fn insertion_order_canonicalises_to_sorted_keys() {
    // serde_json::Value parses into a BTreeMap, so keys round-trip in
    // sorted order regardless of source ordering.
    let v: serde_json::Value = serde_json::from_str(VECTOR_PAYLOAD_INSERTION_ORDER).unwrap();
    let canon = serde_json::to_vec(&v).unwrap();
    assert_eq!(canon, EXPECTED_CANONICAL_PAYLOAD);
}

/// Expected ed25519 public key for the fixed seed. Derived from
/// `[0x07; 32]` via ed25519-dalek.
const EXPECTED_AGENT_PUBKEY_HEX: &str =
    "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";

/// Expected signature over the fixed payload + identity + UUID + timestamp.
/// If this hex changes, either the ed25519 implementation or the canonical
/// encoding has drifted.
const EXPECTED_SIGNATURE_HEX: &str = "b5d93fe27caf9c40a7efb5ae12ddc6ab7e7537a60968545939aba727d1ba5094e8896c47a9d482a261c28ba3b25ce6ea7e89db1b01eaa733e2387dfd5b09ed0c";

/// Expected SHA-256 of the canonical payload bytes (also the task_hash
/// and output_hash on the vector attestation).
const EXPECTED_OUTPUT_HASH_HEX: &str =
    "656512b26e98e00ffc465a37ef446e953665caa3270f6a692eed3ebb308dace4";

/// The full signing_payload bytes — every Attestation field except
/// `signature`, in canonical (struct-order) JSON form. The Python test
/// asserts byte equality against this exact value.
const EXPECTED_SIGNING_PAYLOAD_HEX: &str = "7b226964223a2231313131313131312d323232322d343333332d383434342d353535353535353535353535222c226167656e745f6964223a226167656e742d766563746f72222c226167656e745f7075626b6579223a2265613461366336336532396335323061626566353530376231333265633566393935343737366165626562653762393234323165656136393134343664323263222c227461736b5f68617368223a2236353635313262323665393865303066666334363561333765663434366539353336363563616133323730663661363932656564336562623330386461636534222c226f75747075745f68617368223a2236353635313262323665393865303066666334363561333765663434366539353336363563616133323730663661363932656564336562623330386461636534222c226f7574707574223a227b5c22616374696f6e5c223a5c22617070726f76655c222c5c22616d6f756e745f63656e74735c223a35303030302c5c226e65737465645c223a7b5c22615c223a5b312c322c335d2c5c22795c223a747275657d2c5c227a5f6669656c645c223a5c226c61737420616c7068616265746963616c6c795c227d222c22737461747573223a22436f6d706c65746564222c227265666572656e636573223a6e756c6c2c2274696d657374616d70223a313730303030303030307d";

#[test]
fn vector_signing_payload_is_pinned() {
    let att = build_vector_attestation();
    let actual_hex = hex::encode(att.signing_payload());
    assert_eq!(
        actual_hex, EXPECTED_SIGNING_PAYLOAD_HEX,
        "signing_payload hex drifted.\nIf this change is intentional, update EXPECTED_SIGNING_PAYLOAD_HEX in both this file and crates/awp-python/tests/cross_language.py."
    );
}

#[test]
fn vector_agent_pubkey_is_pinned() {
    let att = build_vector_attestation();
    assert_eq!(hex::encode(att.agent_pubkey), EXPECTED_AGENT_PUBKEY_HEX);
}

#[test]
fn vector_output_hash_is_pinned() {
    let att = build_vector_attestation();
    assert_eq!(hex::encode(att.output_hash), EXPECTED_OUTPUT_HASH_HEX);
    assert_eq!(hex::encode(att.task_hash), EXPECTED_OUTPUT_HASH_HEX);
}

#[test]
fn vector_signature_is_pinned() {
    let att = build_vector_attestation();
    assert_eq!(hex::encode(att.signature), EXPECTED_SIGNATURE_HEX);
}

#[test]
fn vector_signature_verifies() {
    let att = build_vector_attestation();
    assert!(verify_attestation_signature(&att));
}

/// Helper: prints the current signing_payload hex + signature hex for the
/// fixed vector. Run with `cargo test -p awp-core --test cross_language_vector -- print_vector --nocapture --ignored`
/// to regenerate the constants after an intentional canonical-encoding change.
#[test]
#[ignore]
fn print_vector() {
    let att = build_vector_attestation();
    println!(
        "CANONICAL_PAYLOAD_HEX: {}",
        hex::encode(EXPECTED_CANONICAL_PAYLOAD)
    );
    println!(
        "SIGNING_PAYLOAD_HEX:   {}",
        hex::encode(att.signing_payload())
    );
    println!("AGENT_PUBKEY_HEX:      {}", hex::encode(att.agent_pubkey));
    println!("SIGNATURE_HEX:         {}", hex::encode(att.signature));
    println!(
        "ATTESTATION_JSON:      {}",
        serde_json::to_string(&att).unwrap()
    );
}
