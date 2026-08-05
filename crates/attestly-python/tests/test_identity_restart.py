"""Cross-process identity persistence regression test.

Guards `docs/PRODUCTION_CHECKLIST.md` §3 "Persistent agent identity": a
settlement system cannot use receipts whose signing keys vanish on restart, so
the SDK default path
(`python/attestly-langgraph/attestly/langgraph/wrapper.py:120` →
`AgentIdentity.load_or_create(<identities_dir>, agent_id)`) must yield the same
pubkey in a **fresh interpreter**. If that call is ever swapped for the
ephemeral `AgentIdentity.generate(agent_id)`, keys rotate every process and the
tests here fail — which is the point. Deleting them is not free.

`test_api.py::test_load_or_create_persists_across_calls` covers repeated calls
inside one process; only a real process boundary catches an in-memory-only key,
so each pubkey below comes from a separately spawned `sys.executable`.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest


# Runs in a *fresh* interpreter: resolve the identity through the same
# `load_or_create` entry point the SDK uses and print its pubkey as hex.
_PRINT_PUBKEY_SNIPPET = """
import sys
from attestly import _native

identity = _native.AgentIdentity.load_or_create(sys.argv[1], sys.argv[2])
sys.stdout.write(identity.public_key.hex())
"""


def _pubkey_in_fresh_process(identities_dir: Path, agent_id: str) -> str:
    """Spawn a new interpreter that loads `agent_id` and returns its pubkey hex.

    Uses `sys.executable` so the subprocess inherits the same venv and the
    `attestly._native` extension module resolves exactly as it does here.
    """
    proc = subprocess.run(
        [sys.executable, "-c", _PRINT_PUBKEY_SNIPPET, str(identities_dir), agent_id],
        capture_output=True,
        timeout=60,
        check=False,
    )
    assert proc.returncode == 0, (
        f"identity subprocess failed for agent_id={agent_id!r}. "
        f"stderr={proc.stderr.decode(errors='replace')}"
    )
    pubkey_hex = proc.stdout.decode().strip()
    assert len(bytes.fromhex(pubkey_hex)) == 32, f"unexpected pubkey: {pubkey_hex!r}"
    return pubkey_hex


def test_identity_survives_restart(tmp_path: Path) -> None:
    """Two separate processes sharing an identities dir get the same pubkey.

    This is the restart guarantee: the first process creates
    `<tmp_path>/restart-agent.json`, the second — with no shared memory —
    must load that same key off disk. `generate()` would produce a fresh
    random key in each process and this assertion would fail.
    """
    first = _pubkey_in_fresh_process(tmp_path, "restart-agent")
    assert not (Path("data") / "identities" / "restart-agent.json").exists(), (
        "test must stay hermetic — identities belong under tmp_path, "
        "never in the real data/identities/"
    )
    assert (tmp_path / "restart-agent.json").exists(), (
        "load_or_create did not persist the identity to disk"
    )

    second = _pubkey_in_fresh_process(tmp_path, "restart-agent")
    assert first == second, (
        "agent identity did not survive a restart: two fresh processes using "
        "load_or_create produced different pubkeys "
        f"({first} != {second}). Did the default path swap to generate()?"
    )


def test_restart_identity_is_per_agent(tmp_path: Path) -> None:
    """A different agent_id in a third process yields a different pubkey.

    Without this, a `load_or_create` that returned one constant key for every
    agent would still pass the restart assertion above.
    """
    agent = _pubkey_in_fresh_process(tmp_path, "restart-agent")
    other = _pubkey_in_fresh_process(tmp_path, "restart-agent-other")
    assert agent != other, (
        "distinct agent_ids under the same store must have distinct keys"
    )


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
