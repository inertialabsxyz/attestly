// Headless tests for tools/audit-viewer.
//
// Two checks, both critical for the in-browser verification claim:
//
//   1. Static parity: signingPayloadBytes() produces the exact JSON byte
//      sequence we expect from `Attestation::signing_payload` in
//      crates/attestly-core/src/attestation.rs.
//
//   2. End-to-end: every signature in data/attestations.json verifies under
//      the vendored ed25519 implementation, using signingPayloadBytes() as
//      the message. A single byte-level disagreement with Rust's canonical
//      encoding would fail every signature here.
//
// Run with:  node tools/audit-viewer/tests/signing-payload.test.js
// Exit 0 on pass, 1 on any failure.

"use strict";

const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const attestationsPath = path.join(repoRoot, "data", "attestations.json");

const { signingPayloadBytes } = require("../signing-payload.js");
const ed25519 = require("../vendor/ed25519.js");

function fail(msg) {
    console.error("FAIL:", msg);
    process.exitCode = 1;
}
function pass(msg) {
    console.log("PASS:", msg);
}

function bytesEqual(a, b) {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
    return true;
}

// ---- Test 1: static parity ----------------------------------------------

const fixture = {
    id: "00000000-0000-0000-0000-000000000001",
    agent_id: "test-agent",
    agent_pubkey:
        "0000000000000000000000000000000000000000000000000000000000000000",
    task_hash:
        "1111111111111111111111111111111111111111111111111111111111111111",
    output_hash:
        "2222222222222222222222222222222222222222222222222222222222222222",
    output: "84",
    status: "Completed",
    references: null,
    timestamp: 1700000000,
    signature:
        "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
};

const expectedJson =
    '{"id":"00000000-0000-0000-0000-000000000001",' +
    '"agent_id":"test-agent",' +
    '"agent_pubkey":"0000000000000000000000000000000000000000000000000000000000000000",' +
    '"task_hash":"1111111111111111111111111111111111111111111111111111111111111111",' +
    '"output_hash":"2222222222222222222222222222222222222222222222222222222222222222",' +
    '"output":"84",' +
    '"status":"Completed",' +
    '"references":null,' +
    '"timestamp":1700000000}';

{
    const got = signingPayloadBytes(fixture);
    const wanted = new TextEncoder().encode(expectedJson);
    if (bytesEqual(got, wanted)) {
        pass("static fixture: signing payload bytes match expected canonical JSON");
    } else {
        fail(
            "static fixture mismatch.\nWanted: " +
                expectedJson +
                "\nGot:    " +
                new TextDecoder().decode(got),
        );
    }
}

// ---- Test 2: end-to-end against real Rust-signed receipts ----------------
//
// If signingPayloadBytes diverged from Rust by even one byte, none of these
// signatures would verify.

(async () => {
    if (!fs.existsSync(attestationsPath)) {
        console.log(
            "SKIP: " +
                attestationsPath +
                " not found. Run `cargo run --example dispatcher_flow` first.",
        );
        return;
    }
    const lines = fs
        .readFileSync(attestationsPath, "utf8")
        .split(/\r?\n/)
        .filter((l) => l.trim().length > 0);
    if (lines.length === 0) {
        fail("attestations.json is empty — nothing to verify");
        return;
    }
    let okCount = 0;
    for (const line of lines) {
        const att = JSON.parse(line);
        const payload = signingPayloadBytes(att);
        const ok = await ed25519.verify(att.signature, payload, att.agent_pubkey);
        if (!ok) {
            fail(
                "signature did not verify for attestation " +
                    att.id +
                    " (agent=" +
                    att.agent_id +
                    ")",
            );
            return;
        }
        okCount++;
    }
    pass(
        "end-to-end: " +
            okCount +
            " real Rust-signed attestation(s) verified under signingPayloadBytes + vendored ed25519",
    );

    // Tampering test: mutate one byte of output → signature must fail.
    const att = JSON.parse(lines[0]);
    att.output = att.output + "X";
    const payload = signingPayloadBytes(att);
    const ok = await ed25519.verify(att.signature, payload, att.agent_pubkey);
    if (ok) {
        fail("tamper detection: tampered attestation incorrectly verified as valid");
    } else {
        pass("tamper detection: mutated `output` field correctly fails signature verification");
    }
})();
