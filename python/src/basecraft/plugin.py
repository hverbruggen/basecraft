"""Write Basecraft plugins in Python.

A plugin is a script that registers operations and calls ``run()``::

    from basecraft.plugin import Plugin, feature

    plugin = Plugin("me.example", "0.1.0")

    @plugin.operation("count_gc")
    def count_gc(params, context):
        seq = context.snapshot.residues  # numpy uint8 array, no copy
        ...
        return {"ops": [feature("misc_feature", 10, 20, note="here")]}

    plugin.run()

Each operation is declared in the package's ``operations.json``. A handler
receives its validated parameters and a ``Context`` naming the files the host
allocated and, for sequence operations, the document snapshot.
Sequence operations return proposed operations.
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
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = 2
MAX_FDS = 16
MAX_FRAME_BYTES = 8 * 1024 * 1024

Handler = Callable[[dict, "Context"], dict]


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

            mm = mmap.mmap(self._fds[which], 0, access=mmap.ACCESS_READ)
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


class Context:
    """What the host allocated for one run.

    The host sends paths relative to the run folder, or absolute for inputs
    outside it. Every path here is absolute.
    """

    def __init__(self, raw: dict, snapshot: Snapshot | None = None):
        if raw.get("schema") != PROTOCOL_VERSION:
            raise ValueError("operation requires a schema-2 host context")
        root = Path(raw["run_root"])
        self.root = root
        #: The sequence a sequence operation proposes edits to; None otherwise.
        self.snapshot = snapshot
        self.operation: str = raw["operation"]
        self.run_id: str = raw["run_id"]
        self.threads: int = raw["cpu_threads"]
        self.inputs = {name: [str(root / path) for path in paths] for name, paths in raw["inputs"].items()}
        self.outputs = {role: str(root / path) for role, path in raw["outputs"].items()}
        self.cancel_file = str(root / raw["cancel_file"])
        self.progress_file = str(root / raw["progress_file"])
        self.evidence = {
            name: [dict(record, **{key: str(root / record[key]) for key in ("original_path", "run_directory") if record.get(key)})
                   for record in records]
            for name, records in raw.get("input_evidence", {}).items()
        }

    def input(self, name: str) -> str | None:
        """The one file bound to ``name``, or None when the binding is empty."""
        paths = self.inputs.get(name, [])
        if len(paths) > 1:
            raise ValueError(f"{name} requires one file")
        return paths[0] if paths else None

    def output(self, role: str, member: str = ".") -> str:
        """The allocated path for ``role``, or a member inside a collection."""
        target = Path(self.outputs[role])
        if member != ".":
            target = target / member
            target.parent.mkdir(parents=True, exist_ok=True)
        return str(target)

    def cancelled(self) -> bool:
        return Path(self.cancel_file).exists()

    def source(self, name: str) -> dict:
        """Evidence for the first file bound to ``name``, with ``path`` naming a
        stable copy: the original file, or the bytes the host kept for an
        unsaved document. Empty when the host recorded none."""
        records = self.evidence.get(name, [])
        if not records:
            return {}
        record = records[0]
        stable = record.get("original_path")
        if not stable and record.get("run_directory"):
            kept = Path(record["run_directory"]) / "submitted.fasta"
            stable = str(kept) if kept.is_file() else None
        return dict(record, path=stable) if stable else dict(record)


def local_context(root, inputs: dict, outputs: dict, run_id: str = "local", snapshot=None) -> Context:
    """A context for calling a handler directly, as a plugin's own tests do.
    ``inputs`` maps bindings to lists of paths; ``outputs`` maps roles to
    paths relative to ``root``."""
    return Context({"schema": PROTOCOL_VERSION, "operation": "local", "run_id": run_id, "run_root": str(root),
                    "inputs": inputs, "outputs": outputs, "cpu_threads": 1,
                    "cancel_file": "cancel", "progress_file": "progress"}, snapshot)


def _artifacts(context: Context) -> list[dict]:
    """Every file the handler left in its allocated outputs."""
    artifacts = []
    for role, supplied in context.outputs.items():
        root = Path(supplied)
        if root.is_dir():
            for member in sorted(root.rglob("*")):
                if member.is_symlink():
                    raise ValueError("symlink output is not allowed")
                if member.is_file():
                    artifacts.append({"role": role, "path": str(member.relative_to(root))})
        elif root.is_file():
            artifacts.append({"role": role, "path": "."})
    return artifacts


class Plugin:
    def __init__(self, plugin_id: str, version: str):
        self.id = plugin_id
        self.version = version
        self._ops: dict[str, Callable] = {}
        self._queries: dict[str, Callable[[], Any]] = {}

    def operation(self, name: str) -> Callable[[Handler], Handler]:
        """Register a handler ``(parameters, context)`` for a declared operation.
        It returns a dict: ``data``, ``ops`` (sequence operations), ``message``,
        ``tool``, ``execution`` and, optionally, ``artifacts``."""
        def register(fn: Handler) -> Handler:
            if name in self._ops:
                raise ValueError(f"duplicate operation {name!r}")
            self._ops[name] = fn
            return fn

        return register

    def query(self, name: str) -> Callable:
        """Register a metadata query ``()``, such as which engines are installed.
        Queries take no inputs and produce no artifacts."""
        def register(fn: Callable[[], Any]) -> Callable[[], Any]:
            self._queries[name] = fn
            return fn

        return register

    def run(self) -> None:
        portable = "BASECRAFT_TCP" in os.environ
        if portable:
            host, port = os.environ["BASECRAFT_TCP"].rsplit(":", 1)
            if host != "127.0.0.1":
                raise ValueError("plugin transport must be loopback")
            token = os.environ.pop("BASECRAFT_TOKEN").encode("ascii")
            if len(token) != 52:
                raise ValueError("invalid plugin transport token")
            sock = socket.create_connection((host, int(port)), timeout=20)
            sock.sendall(token)
            sock.settimeout(None)
        else:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.connect(os.environ["BASECRAFT_SOCKET"])
        buf = b""
        fds: list[int] = []
        while True:
            while b"\n" not in buf:
                if portable:
                    data, new_fds = sock.recv(1 << 16), []
                else:
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
            if not isinstance(n, int) or not 0 <= n <= MAX_FDS:
                raise ValueError("invalid buffer count")
            if portable:
                buffers = msg.get("buffers", [])
                if not isinstance(buffers, list) or len(buffers) != n:
                    raise ValueError("invalid snapshot buffers")
                mine = []
                try:
                    for path in buffers:
                        if not isinstance(path, str) or not os.path.isabs(path):
                            raise ValueError("invalid snapshot path")
                        mine.append(os.open(path, os.O_RDONLY | getattr(os, "O_BINARY", 0)))
                except BaseException:
                    for fd in mine:
                        os.close(fd)
                    raise
            else:
                if n > len(fds):
                    raise ValueError("missing snapshot buffers")
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
            return {"protocol": PROTOCOL_VERSION, "id": self.id, "version": self.version,
                    "operations": sorted(self._ops)}
        if method in ("ping", "shutdown"):
            return None
        if method == "query":
            return self._queries[params["name"]]()
        if method == "invoke":
            name = params["operation"]
            if name not in self._ops:
                raise KeyError(f"no operation {name!r}")
            snap = Snapshot(params["snapshot"], fds) if params.get("snapshot") else None
            try:
                context = Context(params["context"], snap)
                out = self._ops[name](params.get("params") or {}, context)
            finally:
                if snap is not None:
                    snap.close()
            if not isinstance(out, dict):
                raise TypeError(f"operation {name!r} must return a dict")
            if "artifacts" not in out:
                out = dict(out, artifacts=_artifacts(context))
            return out
        raise ValueError(f"unknown method {method!r}")


def log(*args: Any) -> None:
    """Print to the host's stderr; stdout is reserved."""
    print(*args, file=sys.stderr, flush=True)
