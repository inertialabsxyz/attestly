"""Python bindings for the Agent Work Protocol (AWP).

Public API:

- ``AgentIdentity`` — load or generate a persistent ed25519 identity.
- ``Attestation`` — a signed record of agent work.
- ``sign_attestation(payload, identity)`` — canonicalise ``payload`` and sign.
- ``verify_attestation(attestation, pubkey)`` — verify signature + pubkey match.

All canonical encoding happens in the underlying Rust extension so that
Python-signed attestations verify byte-identically in Rust (and vice
versa). See the README for the cross-language signing guarantee and the
fixed-seed test vector that pins it.
"""

from ._native import (  # type: ignore[attr-defined]
    AgentIdentity,
    Attestation,
    __version__,
    canonical_payload_bytes,
    sign_attestation,
    verify_attestation,
)

__all__ = [
    "AgentIdentity",
    "Attestation",
    "__version__",
    "canonical_payload_bytes",
    "sign_attestation",
    "verify_attestation",
]
