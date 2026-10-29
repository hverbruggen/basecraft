"""Write Basecraft plugins in Python.

A plugin is a script that registers operations and calls ``run()``::

    from basecraft.plugin import Plugin, feature

    plugin = Plugin("me.example", "0.1.0")

    @plugin.operation("count_gc")
    def count_gc(snapshot, params):
        seq = snapshot.residues          # numpy uint8 array, no copy
        ...
        return [feature("misc_feature", 10, 20, note="here")]

    plugin.run()

Operations receive an immutable snapshot and return proposed operations.
Basecraft applies them, maps them onto newer versions if the document
changed in the meantime, logs them and makes them undoable.
"""

from __future__ import annotations

import json
import mmap
import os
import socket
import sys
from collections.abc import Callable
from typing import Any

PROTOCOL_VERSION = 1
MAX_FDS = 16
MAX_FRAME_BYTES = 8 * 1024 * 1024
CONTRACT_CAPABILITY = "operation-contract-v1"

Handler = Callable[["Snapshot | None", dict], "list[dict] | dict | None"]


class Snapshot:
    """A sequence document at one version, mapped from shared memory."""

    def __init__(self, info: dict, fds: list[int]):
        self.doc: str = info["doc"]
        self.name: str = info["name"]
        self.version: int = info["version"]
        self.length: int = info["len"]
        self.circular: bool = info["circular"]
        self._fds = {"residues": fds[info["residues"]], "features": fds[info["features"]]}
        self._maps: dict[str, mmap.mmap] = {}
        self._tables: dict[str, Any] = {}

    def _table(self, which: str):
        if which not in self._tables:
            import pyarrow as pa
            from pyarrow import ipc

            mm = mmap.mmap(self._fds[which], 0, prot=mmap.PROT_READ)
            self._maps[which] = mm
            self._tables[which] = ipc.open_stream(pa.py_buffer(mm)).read_all()
        return self._tables[which]

    @property
    def residues(self):
        """Residues as a read-only numpy ``uint8`` array over shared memory."""
        column = self._table("residues").column("residues")
        return column.chunk(0).to_numpy(zero_copy_only=True)

    @property
    def features(self):
        """Features as a pyarrow Table, one row per segment."""
        return self._table("features")

    def close(self) -> None:
        self._tables.clear()
        for mm in self._maps.values():
            try:
                mm.close()
            except BufferError:
                # A numpy view from `residues` still holds the buffer.
                pass
        for fd in self._fds.values():
            os.close(fd)


def feature(kind: str, start: int, end: int, strand: str = "+", **qualifiers: str) -> dict:
    """A ``feature.add`` proposal. Coordinates are 0-based, half-open."""
    strand_name = {"+": "forward", "-": "reverse"}.get(strand, "unstranded")
    return {
        "op": "feature.add",
        "params": {
            "feature": {
                "kind": kind,
                "location": {"segments": [{"start": start, "end": end, "strand": strand_name}]},
                "qualifiers": [{"key": k, "value": v} for k, v in qualifiers.items()],
            }
        },
    }


class Plugin:
    def __init__(self, plugin_id: str, version: str):
        self.id = plugin_id
        self.version = version
        self._ops: dict[str, Handler] = {}
        self._context_ops: dict[str, Callable] = {}
        self._adapters: dict[str, Callable] = {}

    def operation(self, name: str) -> Callable[[Handler], Handler]:
        def register(fn: Handler) -> Handler:
            if name in self._ops or name in self._context_ops:
                raise ValueError(f"duplicate operation {name!r}")
            self._ops[name] = fn
            return fn

        return register

    def context_operation(self, name: str) -> Callable:
        """Register a schema-2 handler ``(snapshot, parameters, context)``.

        Context contains host-allocated inputs, outputs and controls. Legacy
        two-argument handlers keep their original calling convention.
        """
        def register(fn: Callable) -> Callable:
            if name in self._ops or name in self._context_ops:
                raise ValueError(f"duplicate operation {name!r}")
            self._context_ops[name] = fn
            return fn

        return register

    def contract(self, path="operations.json") -> None:
        """Bind declared compatibility adapters to existing public SDK handlers.

        New plugins can use context_operation directly. This migration adapter
        never changes scientific choices or interprets arbitrary control paths.
        """
        from pathlib import Path

        path = Path(path)
        if not path.is_absolute():
            path = Path(sys.argv[0]).resolve().parent / path
        contract = json.loads(path.read_text())
        if contract["schema"] != 1:
            raise ValueError("unsupported operation contract")
        for descriptor in contract["operations"]:
            name = descriptor["implementation"]
            if name in self._context_ops:
                continue
            handler = self._ops.get(name)
            adapter = descriptor.get("legacy_adapter")
            if handler is None or adapter is None:
                raise ValueError(f"no public handler/adapter for {name!r}")
            self._adapters[name] = self._adapt(handler, adapter)

    @staticmethod
    def _adapt(handler, adapter):
        def invoke(snapshot, parameters, context):
            from pathlib import Path

            effective = dict(parameters)
            controls = context.get("legacy_parameters", {})
            if any(key in effective for key in controls):
                raise ValueError("legacy context overlaps scientific parameters")
            effective.update(controls)
            if controls:
                # Host workflow stage. Its preallocated flat bindings and
                # publication policy stay with the host stage runner; the same
                # scientific schema/defaults and context capability still apply.
                return handler(snapshot, effective)
            for binding, parameter in adapter["inputs"].items():
                paths = context["inputs"].get(binding, [])
                if binding in adapter.get("input_arrays", []):
                    effective[parameter] = paths
                elif paths:
                    if len(paths) != 1:
                        raise ValueError(f"{binding} requires one file")
                    effective[parameter] = paths[0]
            for parameter, binding in adapter["outputs"].items():
                root = Path(context["outputs"][binding["role"]])
                target = root if binding["path"] == "." else root / binding["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                effective[parameter] = str(target)
            effective.update(job_id=context["run_id"], cancel_file=context["cancel_file"], progress=context["progress_file"])
            if allocation := adapter.get("allocation"):
                root = context["outputs"][allocation]
                effective.setdefault("analysis_artifacts", {"metadata_directory": root, "working_directory": root, "run_id": context["run_id"]})
                all_evidence = context.get("input_evidence", {})
                evidence = all_evidence.get(adapter.get("primary_input"), []) if adapter.get("primary_input") else next(iter(all_evidence.values()), [])
                if evidence:
                    record = evidence[0]
                    stable = record.get("original_path")
                    if not stable and record.get("run_directory"):
                        # Unsaved document bytes are retained with the run; name
                        # that copy, never the temporary execution input.
                        retained = Path(record["run_directory"]) / "submitted.fasta"
                        if retained.is_file():
                            stable = str(retained)
                    if stable:
                        effective.setdefault("original_source", stable)
                        effective.setdefault("input_evidence", dict(record, path=stable))
            alignment_binding = next((name for name, parameter in adapter["inputs"].items()
                                      if parameter == "alignment_snapshot"), "alignment")
            alignment = context.get("input_evidence", {}).get(alignment_binding, [])
            if alignment and "alignment_snapshot" in effective:
                record = alignment[0]
                effective.setdefault("alignment_input_hash", record["submitted_hash"])
                effective.setdefault("alignment_evidence", dict(record, path=record.get("original_path") or effective["alignment_snapshot"]))
            output = handler(snapshot, effective)
            if output is None:
                output = {"ops": []}
            elif isinstance(output, list):
                output = {"ops": output}
            else:
                output = dict(output)
            reported = output.pop("outputs", [])
            artifacts = []
            allocated = []
            for role, supplied in context["outputs"].items():
                root = Path(supplied)
                if root.is_dir():
                    for member in sorted(root.rglob("*")):
                        if member.is_symlink():
                            raise ValueError("symlink output is not allowed")
                        if member.is_file():
                            artifacts.append({"role": role, "path": str(member.relative_to(root))})
                            allocated.append(member)
                elif root.is_file():
                    artifacts.append({"role": role, "path": "."})
                    allocated.append(root)
            if any(Path(path) not in allocated for path in reported):
                raise ValueError("handler reported an unallocated artifact")
            output.update(artifacts=artifacts, data=output.get("data"))
            return output

        return invoke

    def run(self) -> None:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(os.environ["BASECRAFT_SOCKET"])
        buf = b""
        fds: list[int] = []
        while True:
            while b"\n" not in buf:
                data, new_fds, _flags, _addr = socket.recv_fds(sock, 1 << 16, MAX_FDS)
                if not data:
                    return
                buf += data
                fds.extend(new_fds)
                if len(buf) > MAX_FRAME_BYTES or len(fds) > MAX_FDS:
                    raise ValueError("plugin protocol frame/descriptor limit exceeded")
            line, buf = buf.split(b"\n", 1)
            msg = json.loads(line)
            n = msg.get("fds", 0)
            mine, fds = fds[:n], fds[n:]
            reply: dict[str, Any] = {"jsonrpc": "2.0", "id": msg.get("id")}
            try:
                reply["result"] = self._dispatch(msg.get("method"), msg.get("params"), mine)
            except Exception as e:  # noqa: BLE001 - report any failure to the host, keep serving
                reply["error"] = {"code": -32000, "message": f"{type(e).__name__}: {e}"}
            sock.sendall(json.dumps(reply).encode() + b"\n")
            if msg.get("method") == "shutdown":
                return

    def _dispatch(self, method: str | None, params: Any, fds: list[int]) -> Any:
        if method == "initialize":
            return {
                "protocol": PROTOCOL_VERSION,
                "id": self.id,
                "version": self.version,
                "operations": sorted(self._ops.keys() | self._context_ops.keys()),
                "capabilities": [CONTRACT_CAPABILITY] if self._context_ops or self._adapters else [],
            }
        if method in ("ping", "shutdown"):
            return None
        if method == "invoke":
            name = params["operation"]
            if name not in self._ops and name not in self._context_ops:
                raise KeyError(f"no operation {name!r}")
            snap = Snapshot(params["snapshot"], fds) if params.get("snapshot") else None
            try:
                if name in self._context_ops or (name in self._adapters and params.get("context") is not None):
                    context = params.get("context")
                    if not isinstance(context, dict) or context.get("schema") != 1:
                        raise ValueError("operation requires operation-contract-v1 host context")
                    handler = self._context_ops.get(name) or self._adapters[name]
                    out = handler(snap, params.get("params") or {}, context)
                else:
                    if params.get("context") is not None:
                        raise ValueError("legacy handler cannot accept operation contract context")
                    out = self._ops[name](snap, params.get("params") or {})
            finally:
                if snap is not None:
                    snap.close()
            if out is None:
                out = []
            if isinstance(out, list):
                out = {"ops": out}
            return out
        raise ValueError(f"unknown method {method!r}")


def log(*args: Any) -> None:
    """Print to the host's stderr; stdout is reserved."""
    print(*args, file=sys.stderr, flush=True)
