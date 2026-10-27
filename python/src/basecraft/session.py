"""A headless Basecraft project session over the CLI's MCP stdio server.

``Session`` launches ``basecraft mcp <project>`` and speaks the same MCP tools
that external assistants use: document open/read/apply/undo/redo/save, the
document record, installed operations and their owned jobs. It never attaches
to a running GUI and does not see unsaved GUI edits.

Schemas, defaults, permissions, execution and provenance belong to the host.
This module only frames requests and maps structured errors to OperationError.
"""
from __future__ import annotations

import json
import queue
import subprocess
import threading
import time

from .tools import executable

PROTOCOL_VERSION = "2025-11-25"
# Same wire frame limit as basecraft_protocol::operation::MAX_FRAME_BYTES.
MAX_FRAME = 8 * 1024 * 1024
TERMINAL = frozenset({"completed", "failed", "cancelled", "interrupted", "partial"})
_STDERR_TAIL = 64 * 1024
_JOB_TOOL = "basecraft.host.jobs_{}.v1"


class OperationError(RuntimeError):
    """A structured host failure. ``code`` is the host's error code."""

    def __init__(self, outcome):
        self.outcome = outcome
        self.run_id = outcome.get("run_id")
        errors = outcome.get("errors") or [{"code": "tool_failure", "message": str(outcome)}]
        self.code = errors[0]["code"]
        super().__init__(errors[0]["message"])


def _failure(code, message, run_id=None):
    outcome = {"state": "failed", "errors": [{"code": code, "message": message}]}
    if run_id is not None:
        outcome["run_id"] = run_id
    return OperationError(outcome)


def tool_name(operation):
    """MCP tool name for an operation ID: ``plugin/name@1`` -> ``plugin.name.v1``."""
    plugin, _, rest = operation.partition("/")
    name, _, major = rest.rpartition("@")
    if not plugin or not name or not major.isdigit():
        raise _failure("invalid_arguments", f"Operation IDs have the form plugin/name@major: {operation}")
    return f"{plugin}.{name}.v{major}"


def request(operation, *, inputs=None, parameters=None, destinations=None, pins=None,
            request_key=None):
    """Build an OperationRequest. The host fills defaults and validates it."""
    value = {"operation": operation, "inputs": {} if inputs is None else inputs,
             "parameters": {} if parameters is None else parameters,
             "destinations": {} if destinations is None else destinations}
    if pins is not None:
        value["pins"] = pins
    if request_key is not None:
        value["request_key"] = request_key
    return value


class Job:
    """An operation job owned by one session. Waiting never cancels it."""

    def __init__(self, session, accepted):
        self.session = session
        self.accepted = accepted
        self.job = accepted["run_id"]

    def _call(self, kind):
        outcome = self.session.call(_JOB_TOOL.format(kind), {"job": self.job}, check=False)
        if outcome.get("state") in TERMINAL:
            self.session._owned.discard(self.job)
        return outcome

    def status(self):
        """Return the host outcome as is, including failed or cancelled states."""
        return self._call("get")

    def cancel(self):
        return self._call("cancel")

    def repair(self):
        """Retry retained partial publication. Never runs the plugin again."""
        return self._call("repair")

    def result(self, timeout=None):
        """Wait for the terminal outcome. A wait timeout does not cancel the job."""
        started = time.monotonic()
        while self.status().get("state") not in TERMINAL:
            if timeout is not None and time.monotonic() - started >= timeout:
                raise TimeoutError("Operation wait timed out; the session still owns the job")
            time.sleep(0.025)
        outcome = self._call("result")
        if outcome.get("state") != "completed":
            raise OperationError(outcome)
        return outcome


class Session:
    """One headless project session served by ``basecraft mcp``.

    ``writes=False`` starts the read-only server; edits, saves and operations
    that declare writes are refused by the host. Edits are recorded as script
    edits named ``client``, which is also the initiator of operation runs.
    ``timeout`` bounds each request, not the jobs it starts.
    Closing the session cancels jobs it still owns.
    """

    def __init__(self, project=".", *, writes=False, client="Python", timeout=60.0,
                 command=None):
        # Edits from this session are recorded as script edits, not agent edits.
        arguments = [command or executable(["basecraft"], "BASECRAFT"), "mcp", str(project),
                     "--client-kind", "script", "--client", client]
        if writes:
            arguments.append("--allow-writes")
        self.writes = writes
        self.timeout = timeout
        self.notifications = []
        self._owned = set()
        self._tools = None
        self._serial = 0
        self._lock = threading.RLock()
        self._messages = queue.Queue()
        self._stderr = bytearray()
        self._process = subprocess.Popen(arguments, stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        threading.Thread(target=self._read_stdout, daemon=True).start()
        self._stderr_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self._stderr_reader.start()
        try:
            self.info = self._request("initialize", {
                "protocolVersion": PROTOCOL_VERSION, "capabilities": {},
                "clientInfo": {"name": "basecraft-python", "version": "1"}})
            self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise

    # Transport

    def _read_stdout(self):
        stream = self._process.stdout
        try:
            while True:
                line = stream.readline(MAX_FRAME + 1)
                if not line:
                    self._messages.put(("closed", None))
                    return
                if len(line) > MAX_FRAME or not line.endswith(b"\n"):
                    self._messages.put(("error", "Host response exceeded the frame limit"))
                    return
                self._messages.put(("line", line))
        except (OSError, ValueError) as error:
            self._messages.put(("error", str(error)))

    def _read_stderr(self):
        try:
            for chunk in iter(lambda: self._process.stderr.read1(65536), b""):
                self._stderr.extend(chunk)
                del self._stderr[:-_STDERR_TAIL]
        except (OSError, ValueError):
            pass

    def _diagnostic(self, message):
        if self._process.poll() is not None:
            self._stderr_reader.join(timeout=1)
            detail = self._stderr.decode(errors="replace").strip()
            if detail:
                return f"{message}: {detail}"
        return message

    def _send(self, message):
        frame = json.dumps(message).encode() + b"\n"
        if len(frame) > MAX_FRAME:
            raise _failure("invalid_arguments", "Request exceeded the host frame limit")
        try:
            self._process.stdin.write(frame)
            self._process.stdin.flush()
        except (OSError, ValueError) as error:
            raise _failure("plugin_protocol_violation",
                           self._diagnostic(f"Basecraft session closed: {error}")) from error

    def _request(self, method, params=None, timeout=None):
        """Send one JSON-RPC request and return its result. Timeouts cancel nothing."""
        timeout = self.timeout if timeout is None else timeout
        with self._lock:
            self._serial += 1
            serial = self._serial
            self._send({"jsonrpc": "2.0", "id": serial, "method": method,
                        "params": {} if params is None else params})
            deadline = None if timeout is None else time.monotonic() + timeout
            while True:
                remaining = None if deadline is None else max(0.0, deadline - time.monotonic())
                try:
                    kind, payload = self._messages.get(timeout=remaining)
                except queue.Empty:
                    raise TimeoutError(f"Basecraft did not answer {method} in {timeout} s; "
                                       "started jobs keep running") from None
                if kind != "line":
                    # Keep the failure visible to later calls on this session.
                    self._messages.put((kind, payload))
                    message = payload or "Basecraft session closed"
                    raise _failure("plugin_protocol_violation", self._diagnostic(message))
                try:
                    reply = json.loads(payload)
                except ValueError as error:
                    raise _failure("plugin_protocol_violation",
                                   f"Malformed host response: {error}") from error
                if not isinstance(reply, dict):
                    raise _failure("plugin_protocol_violation", "Host response must be an object")
                if "id" not in reply:
                    self.notifications.append(reply)
                    if reply.get("method") == "notifications/tools/list_changed":
                        self._tools = None
                    continue
                if reply["id"] != serial:
                    continue  # Late answer to an earlier request that timed out.
                if "error" in reply:
                    error = reply["error"] if isinstance(reply["error"], dict) else {}
                    data = error.get("data")
                    if error.get("code") == -32602 and isinstance(data, dict) \
                            and data.get("errors"):
                        # The host's structured refusal, e.g. an unknown operation.
                        failure = OperationError(data)
                    else:
                        code = "invalid_arguments" if error.get("code") == -32602 else \
                            "plugin_protocol_violation"
                        failure = _failure(code, str(error.get("message", reply["error"])))
                    failure.rpc_code = error.get("code")
                    raise failure
                if not isinstance(reply.get("result"), dict):
                    raise _failure("plugin_protocol_violation", "Host result must be an object")
                return reply["result"]

    # Discovery

    def tools(self):
        """All MCP tool definitions, following pagination."""
        with self._lock:
            if self._tools is None:
                tools, cursor = [], None
                while True:
                    page = self._request("tools/list", {} if cursor is None else {"cursor": cursor})
                    tools.extend(page.get("tools", []))
                    cursor = page.get("nextCursor")
                    if cursor is None:
                        break
                self._tools = tools
            return list(self._tools)

    def operations(self):
        """Installed operations this session may run, with their request/outcome schemas."""
        listed = []
        for tool in self.tools():
            schema = tool.get("inputSchema", {})
            operation = schema.get("properties", {}).get("operation", {}).get("const")
            if operation is None:
                continue
            listed.append({"operation": operation, "tool": tool["name"],
                           "description": tool.get("description"), "input_schema": schema,
                           "output_schema": tool.get("outputSchema"),
                           "annotations": tool.get("annotations", {}),
                           "pins": tool.get("_meta", {}).get("basecraft", {}).get("pins")})
        return listed

    def refresh(self):
        """Reload installed plugin metadata without running plugins."""
        with self._lock:
            self._tools = None
            return self.call("basecraft.host.registry_refresh.v1")

    # Tools

    def call(self, name, arguments=None, *, check=True, timeout=None):
        """Call one MCP tool and return its structured content.

        With ``check`` a tool error raises OperationError carrying the host code.
        """
        result = self._request("tools/call", {"name": name,
                                              "arguments": {} if arguments is None else arguments},
                               timeout)
        value = result.get("structuredContent")
        if value is None:
            try:
                value = json.loads(result["content"][0]["text"])
            except (KeyError, IndexError, TypeError, ValueError) as error:
                raise _failure("plugin_protocol_violation",
                               "Host tool result has no structured content") from error
        if check and result.get("isError"):
            raise OperationError(value if isinstance(value, dict) else {"errors": []})
        return value

    def open(self, path):
        """Open a project document. Returns its state, including the document ID."""
        return self.call("open", {"path": str(path)})

    def documents(self):
        return self.call("documents")

    def state(self, document):
        return self.call("state", {"document": document})

    def read(self, document, start=None, end=None, row=None):
        """Read residues ``start..end`` (row for alignments), tree tips or a delimitation."""
        arguments = {"document": document}
        for key, value in (("start", start), ("end", end), ("row", row)):
            if value is not None:
                arguments[key] = value
        return self.call("read", arguments)

    def apply(self, document, version, operation):
        """Apply one typed edit to the version you inspected; logged and undoable."""
        return self.call("apply", {"document": document, "version": version,
                                   "operation": operation})

    def undo(self, document):
        return self.call("undo", {"document": document})

    def redo(self, document):
        return self.call("redo", {"document": document})

    def save(self, document, path=None):
        arguments = {"document": document}
        if path is not None:
            arguments["path"] = str(path)
        return self.call("save", arguments)

    def document_record(self, document):
        """History and methods for this session's current revision, including unsaved edits."""
        return self.call("document_record", {"document": document})

    # Operations

    def submit(self, operation, **arguments):
        """Start an installed operation and return its owned Job promptly."""
        try:
            name = tool_name(operation)
            accepted = self.call(name, request(operation, **arguments))
        except OperationError as error:
            # Hosts that predate structured error.data answer only "Unknown tool".
            if getattr(error, "rpc_code", None) == -32602 and str(error) == "Unknown tool":
                raise _failure("plugin_unavailable", f"Unknown operation {operation}") from error
            raise
        if not isinstance(accepted, dict) or "run_id" not in accepted:
            raise OperationError(accepted if isinstance(accepted, dict) else {"errors": []})
        self._owned.add(accepted["run_id"])
        return Job(self, accepted)

    def run(self, operation, *, timeout=None, **arguments):
        """Run an operation to completion and return its outcome."""
        return self.submit(operation, **arguments).result(timeout=timeout)

    # Lifetime

    def close(self, timeout=30.0):
        """Cancel owned unfinished jobs, then end the server and reap it."""
        process = self._process
        if process.poll() is None:
            for job in sorted(self._owned):
                try:
                    self.call(_JOB_TOOL.format("cancel"), {"job": job}, check=False, timeout=5)
                except (OperationError, TimeoutError):
                    break  # EOF below asks the host to cancel the rest.
        self._owned.clear()
        if process.stdin and not process.stdin.closed:
            try:
                process.stdin.close()
            except OSError:
                pass
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        self._stderr_reader.join(timeout=1)
        for stream in (process.stdout, process.stderr):
            if stream:
                stream.close()

    def __enter__(self):
        return self

    def __exit__(self, *_exception):
        self.close()
