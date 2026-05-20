"""``attest()`` — wrap a LangGraph ``StateGraph`` so every node emits a
signed attestation when it completes.

Implementation strategy: monkey-patch the graph's ``add_node`` so that any
node added afterwards goes through a thin wrapper. Existing nodes added
*before* ``attest()`` runs are re-registered with the wrapper around them.
The graph's behaviour is preserved exactly — node outputs are the wrapped
function's return value, routing keys are unchanged, exceptions propagate.

We wrap the node callable itself rather than relying on LangGraph's
internal callback/listener plumbing because the public ``add_node`` contract
is by far the longest-lived API in the library (it has not changed shape
between 0.2.x and 0.6.x), and decorating a node function does not depend on
internal types.

Signing is delegated entirely to ``awp-core-py``: we build the payload dict
described in ``planning/gtm-phase-2-agent-prompts.md`` § Step 3, hand it to
``awp.sign_attestation``, and the canonical-JSON-and-Ed25519 contract is
provided by Step 1's PyO3 bindings.
"""

from __future__ import annotations

import copy
import hashlib
import inspect
import json
import logging
import os
import time
from functools import wraps
from pathlib import Path
from typing import Any, Callable, Optional

# Import from awp-core-py's C-extension submodule rather than the top-level
# `awp` package: `awp` is a PEP 420 namespace package shared with this SDK,
# so `import awp` may execute either package's `__init__.py`. The `_native`
# submodule is unambiguous regardless of import order.
try:
    from awp import _native as _awp_core  # type: ignore
except ImportError as exc:  # pragma: no cover - awp-core-py is a hard dep
    raise ImportError(
        "awp-langgraph requires awp-core-py to be installed. "
        "Install it with `pip install awp-core-py` (or `maturin develop` "
        "from `crates/awp-python/` for local builds)."
    ) from exc

from awp.langgraph import telemetry as _telemetry
from awp.langgraph.sinks import FileSink, Sink

_log = logging.getLogger("awp.langgraph")

# Marker stashed on a wrapped graph so a second attest() call is a no-op.
_ATTEST_MARKER_ATTR = "__awp_attest_wrapped__"

# Default sink location, mirroring the existing repo's ``data/`` convention.
_DEFAULT_FILE_SINK_PATH = Path("data") / "attestations.jsonl"
_DEFAULT_IDENTITIES_DIR = Path("data") / "identities"


def attest(
    graph: Any,
    agent_id: str,
    *,
    identity: Optional[Any] = None,
    sink: Optional[Sink] = None,
    verifier_agent_id: Optional[str] = None,
    verifier_identity: Optional[Any] = None,
    identities_dir: Optional[os.PathLike] = None,
    clock: Optional[Callable[[], float]] = None,
) -> Any:
    """Wrap a LangGraph ``StateGraph`` so that every node emits a signed
    attestation.

    Parameters
    ----------
    graph:
        A LangGraph ``StateGraph`` (or anything that exposes ``add_node``
        and a ``nodes`` mapping). Wrapping happens in place and the same
        object is returned for chaining.
    agent_id:
        Stable string identifier for the Worker agent. Used to look up the
        persisted identity at ``data/identities/<agent_id>.json``.
    identity:
        Explicit ``awp.AgentIdentity``. Overrides the file-store lookup.
    sink:
        Where to send signed attestations. Defaults to
        ``FileSink(./data/attestations.jsonl)``.
    verifier_agent_id:
        If set, enables dual-agent mode: every node runs twice
        (sequentially in v0.1), the second pass under this identity. The
        Verifier attestation embeds the verdict (``attestation_valid``,
        ``answer_correct``) and references the Worker attestation by id.
        Disagreement is surfaced as a structured warning log line — the
        wrapped graph still runs to completion.
    verifier_identity:
        Explicit Verifier ``awp.AgentIdentity``. Overrides the file-store
        lookup.
    identities_dir:
        Override the on-disk identity directory. Defaults to
        ``./data/identities/``.
    clock:
        Optional time source returning unix seconds. Useful in tests;
        defaults to :func:`time.time`.

    Returns
    -------
    The same graph object, with attestation hooks attached. Calling
    ``attest()`` a second time on the same graph is a no-op.
    """
    if getattr(graph, _ATTEST_MARKER_ATTR, False):
        return graph

    if sink is None:
        sink = FileSink(_DEFAULT_FILE_SINK_PATH)
    if clock is None:
        clock = time.time

    ids_dir = Path(identities_dir) if identities_dir is not None else _DEFAULT_IDENTITIES_DIR
    worker = identity or _awp_core.AgentIdentity.load_or_create(str(ids_dir), agent_id)
    verifier = None
    if verifier_agent_id is not None:
        verifier = verifier_identity or _awp_core.AgentIdentity.load_or_create(
            str(ids_dir), verifier_agent_id
        )

    state = _AttestationState(
        worker=worker,
        worker_id=agent_id,
        verifier=verifier,
        verifier_id=verifier_agent_id,
        sink=sink,
        clock=clock,
    )

    # Re-wrap any nodes that were already added before attest() was called.
    nodes = getattr(graph, "nodes", None)
    if isinstance(nodes, dict):
        for name, node in list(nodes.items()):
            wrapped = _wrap_existing_node(name, node, state)
            if wrapped is not None:
                nodes[name] = wrapped

    # Intercept future add_node calls.
    original_add_node = graph.add_node

    @wraps(original_add_node)
    def attesting_add_node(*args, **kwargs):
        name, fn, rest_args, rest_kwargs = _split_add_node_args(args, kwargs)
        wrapped_fn = _wrap_node_callable(name, fn, state)
        if name is None:
            return original_add_node(wrapped_fn, *rest_args, **rest_kwargs)
        return original_add_node(name, wrapped_fn, *rest_args, **rest_kwargs)

    graph.add_node = attesting_add_node  # type: ignore[attr-defined]
    setattr(graph, _ATTEST_MARKER_ATTR, True)
    setattr(graph, "_awp_state", state)
    return graph


# ---------------------------------------------------------------------------
# Internals
# ---------------------------------------------------------------------------


class _AttestationState:
    """Bag of identities + sink + clock shared by every wrapped node."""

    __slots__ = ("worker", "worker_id", "verifier", "verifier_id", "sink", "clock")

    def __init__(
        self,
        worker: Any,
        worker_id: str,
        verifier: Optional[Any],
        verifier_id: Optional[str],
        sink: Sink,
        clock: Callable[[], float],
    ) -> None:
        self.worker = worker
        self.worker_id = worker_id
        self.verifier = verifier
        self.verifier_id = verifier_id
        self.sink = sink
        self.clock = clock


def _split_add_node_args(args, kwargs):
    """Best-effort parse of ``add_node`` arguments across LangGraph versions.

    Supports both ``add_node("name", fn, ...)`` and ``add_node(fn)``.
    """
    name: Optional[str] = None
    fn: Optional[Callable] = None
    rest_args: tuple = ()
    rest_kwargs: dict = dict(kwargs)

    if "node" in rest_kwargs and "name" in rest_kwargs:
        name = rest_kwargs.pop("name")
        fn = rest_kwargs.pop("node")
    elif "action" in rest_kwargs and "name" in rest_kwargs:
        name = rest_kwargs.pop("name")
        fn = rest_kwargs.pop("action")
    elif args:
        if isinstance(args[0], str):
            name = args[0]
            if len(args) >= 2 and callable(args[1]):
                fn = args[1]
                rest_args = tuple(args[2:])
            else:
                rest_args = tuple(args[1:])
        elif callable(args[0]):
            fn = args[0]
            name = getattr(fn, "__name__", None)
            rest_args = tuple(args[1:])

    if fn is None:
        raise TypeError(
            "attest(): could not interpret add_node arguments; expected "
            "add_node(name, fn) or add_node(fn)"
        )
    return name, fn, rest_args, rest_kwargs


def _wrap_existing_node(name: str, node: Any, state: _AttestationState) -> Optional[Any]:
    """Wrap the callable inside a LangGraph node-spec object in place.

    LangGraph wraps user node callables in different shells depending on
    version:

    - LangGraph >=0.3 uses ``StateNodeSpec.runnable`` where ``runnable`` is
      a ``RunnableCallable`` with both ``.func`` (sync) and ``.afunc``
      (async) attributes pointing at the user's function.
      ``callable(runnable)`` returns ``False``; the underlying user
      callable lives at ``.func`` / ``.afunc``.
    - LangGraph 0.2.x stored a plain callable under ``.runnable`` (or
      ``.func`` / ``.action`` / ``.node`` depending on minor version).

    The cascade below covers all the shapes we've seen. If none match the
    node is left untouched and a warning is logged so a user can still see
    which nodes are not being attested.
    """
    inner_shell = getattr(node, "runnable", None)
    if inner_shell is not None and (
        hasattr(inner_shell, "func") or hasattr(inner_shell, "afunc")
    ):
        wrapped_any = False
        sync_fn = getattr(inner_shell, "func", None)
        if callable(sync_fn):
            try:
                inner_shell.func = _wrap_node_callable(name, sync_fn, state)
                wrapped_any = True
            except (AttributeError, TypeError):
                pass
        async_fn = getattr(inner_shell, "afunc", None)
        if callable(async_fn):
            try:
                inner_shell.afunc = _wrap_node_callable(name, async_fn, state)
                wrapped_any = True
            except (AttributeError, TypeError):
                pass
        if wrapped_any:
            return node

    for attr in ("runnable", "func", "action", "node"):
        inner = getattr(node, attr, None)
        if callable(inner):
            try:
                setattr(node, attr, _wrap_node_callable(name, inner, state))
                return node
            except (AttributeError, TypeError):
                continue

    if callable(node):
        return _wrap_node_callable(name, node, state)

    _log.warning(
        "awp.langgraph: could not find callable inside node %r — "
        "this node will not emit attestations",
        name,
    )
    return None


def _wrap_node_callable(
    name: Optional[str],
    fn: Callable,
    state: _AttestationState,
) -> Callable:
    """Wrap a node callable with attestation emission.

    The wrapper preserves async vs sync, the return value, and exception
    propagation. Attestations are emitted *after* the node returns; if the
    node raises, no attestation is emitted (a partial result is worse than
    none — the audit log should only contain receipts for work that
    completed).
    """
    node_name = name or getattr(fn, "__name__", "anonymous")

    if inspect.iscoroutinefunction(fn):

        @wraps(fn)
        async def async_wrapper(input_state, *args, **kwargs):
            input_snapshot = _snapshot_state(input_state)
            output = await fn(input_state, *args, **kwargs)

            verifier_output: Any = None
            if state.verifier is not None:
                replay_input = _snapshot_state(input_snapshot)
                verifier_output = await fn(replay_input, *args, **kwargs)

            _emit_for_node(
                node_name, input_snapshot, output, verifier_output, state
            )
            return output

        return async_wrapper

    @wraps(fn)
    def sync_wrapper(input_state, *args, **kwargs):
        input_snapshot = _snapshot_state(input_state)
        output = fn(input_state, *args, **kwargs)

        verifier_output: Any = None
        if state.verifier is not None:
            replay_input = _snapshot_state(input_snapshot)
            verifier_output = fn(replay_input, *args, **kwargs)

        _emit_for_node(node_name, input_snapshot, output, verifier_output, state)
        return output

    return sync_wrapper


def _snapshot_state(state: Any) -> Any:
    """Deep-copy the input so a node mutating in place doesn't poison our
    input-hash computation. Skip the copy if the value is a primitive.
    """
    if isinstance(state, (str, int, float, bool, type(None))):
        return state
    try:
        return copy.deepcopy(state)
    except Exception:
        # Non-copyable inputs are rare in LangGraph state but possible
        # (e.g. open file handles). Fall back to repr-based hashing.
        return repr(state)


def _emit_for_node(
    node_name: str,
    input_snapshot: Any,
    worker_output: Any,
    verifier_output: Any,
    state: _AttestationState,
) -> None:
    """Build, sign, and emit Worker (and optionally Verifier) attestations.

    Both attestations are signed via ``awp.sign_attestation``, which owns
    the canonical-JSON-and-Ed25519 contract. The payload dict matches the
    shape called out in the Step 3 prompt: ``{node_name, input_hash,
    output_hash, agent_id, timestamp}``.
    """
    timestamp = int(state.clock())
    input_hash_hex = _stable_hash(input_snapshot)
    worker_output_hash_hex = _stable_hash(worker_output)

    worker_payload = {
        "node_name": node_name,
        "input_hash": input_hash_hex,
        "output_hash": worker_output_hash_hex,
        "agent_id": state.worker_id,
        "timestamp": timestamp,
    }
    worker_att = _awp_core.sign_attestation(
        worker_payload, state.worker, timestamp=timestamp
    )
    state.sink.emit(worker_att)
    _telemetry.record_attestation(sink_type=_sink_type_of(state.sink))

    if state.verifier is None:
        return

    verifier_output_hash_hex = _stable_hash(verifier_output)
    outputs_agree = verifier_output_hash_hex == worker_output_hash_hex

    # The Verifier cryptographically re-verifies the Worker's attestation
    # rather than assuming it is valid — `attestation_valid` is a real
    # verdict, not a placeholder.
    try:
        attestation_valid = bool(
            _awp_core.verify_attestation(worker_att, worker_att.agent_pubkey)
        )
    except Exception:  # pragma: no cover - defensive; verify shouldn't raise
        attestation_valid = False

    verifier_payload = {
        "node_name": node_name,
        "input_hash": input_hash_hex,
        "output_hash": verifier_output_hash_hex,
        "agent_id": state.verifier_id,
        "timestamp": timestamp,
        # Verdict embedded in the payload itself so it is covered by the
        # signature. The Attestation.status field is "Completed" because
        # awp-core-py v0.1 only emits Completed; the audit trail records
        # the verdict via these two keys instead.
        "references": worker_att.id,
        "attestation_valid": attestation_valid,
        "answer_correct": outputs_agree,
    }
    verifier_att = _awp_core.sign_attestation(
        verifier_payload, state.verifier, timestamp=timestamp
    )
    state.sink.emit(verifier_att)
    _telemetry.record_attestation(sink_type=_sink_type_of(state.sink))

    if not outputs_agree:
        _log.warning(
            "awp.langgraph: Worker/Verifier disagreement on node=%s "
            "worker_id=%s verifier_id=%s worker_output_hash=%s "
            "verifier_output_hash=%s",
            node_name,
            state.worker_id,
            state.verifier_id,
            worker_output_hash_hex,
            verifier_output_hash_hex,
        )


def _stable_hash(value: Any) -> str:
    """SHA-256 of a stable string rendering of ``value``.

    Prefers JSON (sorted keys, no whitespace) for the common dict/list/
    scalar cases LangGraph state produces; falls back to ``repr()`` for
    things that aren't JSON-serialisable. Within a single process the
    repr is stable enough for the input/output hash pair to detect real
    changes.
    """
    h = hashlib.sha256()
    h.update(_stable_repr(value).encode("utf-8"))
    return h.hexdigest()


def _stable_repr(value: Any) -> str:
    if isinstance(value, (str, int, float, bool, type(None))):
        return json.dumps(value, separators=(",", ":"))
    try:
        return json.dumps(value, separators=(",", ":"), sort_keys=True, default=repr)
    except (TypeError, ValueError):
        return repr(value)


def _sink_type_of(sink: Any) -> str:
    """Best-effort sink-type label for the telemetry counter."""
    label = getattr(sink, "sink_type", None)
    if isinstance(label, str):
        return label
    return type(sink).__name__
