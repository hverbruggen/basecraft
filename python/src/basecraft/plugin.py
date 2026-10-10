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

    def operation(self, name: str) -> Callable[[Handler], Handler]:
        def register(fn: Handler) -> Handler:
            self._ops[name] = fn
            return fn

        return register

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
                "operations": sorted(self._ops),
            }
        if method in ("ping", "shutdown"):
            return None
        if method == "invoke":
            name = params["operation"]
            if name not in self._ops:
                raise KeyError(f"no operation {name!r}")
            snap = Snapshot(params["snapshot"], fds) if params.get("snapshot") else None
            try:
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
