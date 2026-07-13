// Reconstruction of `Attestation::signing_payload` (see
// `crates/attestly-core/src/attestation.rs`) in JavaScript.
//
// Rust source-of-truth structure:
//
//   #[derive(Serialize)]
//   struct SigningPayload<'a> {
//       id, agent_id, agent_pubkey, task_hash, output_hash,
//       output, status, references, timestamp,
//   }
//
// `agent_pubkey`, `task_hash`, `output_hash` are 32-byte arrays serialized
// as hex strings (already-hex on the wire). `signature` is excluded.
// serde_json preserves struct field order, so the canonical bytes are the
// JSON encoding of an object with those keys in that exact order.

(function (root, factory) {
    "use strict";
    const api = factory();
    if (typeof module === "object" && module.exports) {
        module.exports = api;
    } else {
        root.signingPayload = api;
    }
})(typeof self !== "undefined" ? self : this, function () {
    "use strict";

    function signingPayloadBytes(att) {
        const view = {
            id: att.id,
            agent_id: att.agent_id,
            agent_pubkey: att.agent_pubkey,
            task_hash: att.task_hash,
            output_hash: att.output_hash,
            output: att.output,
            status: att.status,
            references: att.references === undefined ? null : att.references,
            timestamp: att.timestamp,
        };
        return new TextEncoder().encode(JSON.stringify(view));
    }

    return { signingPayloadBytes: signingPayloadBytes };
});
