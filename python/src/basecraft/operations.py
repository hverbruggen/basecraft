"""Installed operations through the shared host, using independent headless sessions.

Schemas, defaults, permissions, execution and provenance belong to the host.
Closing a live handle cancels its owned job session; it never attaches to a GUI.
"""
from __future__ import annotations

import json
import subprocess
import threading
import time

from .tools import executable

_MAX_FRAME = 8 * 1024 * 1024
_TERMINAL = {"completed", "failed", "cancelled", "interrupted", "partial"}


class OperationError(RuntimeError):
    def __init__(self, outcome):
        self.outcome = outcome
        self.run_id = outcome.get("run_id")
        errors = outcome.get("errors") or [{"code": "tool_failure", "message": str(outcome)}]
        self.code = errors[0]["code"]
        super().__init__(errors[0]["message"])


def _command():
    return executable(["basecraft"], "BASECRAFT")


def _metadata(arguments):
    result = subprocess.run([_command(), "operations", *arguments, "--json"],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise OperationError({"errors": [{"code": "plugin_unavailable", "message": result.stderr.strip()}]})
    return json.loads(result.stdout)


def list():
    """Return the current registry generation, descriptors and diagnostics."""
    return _metadata(["list"])


def describe(operation):
    return _metadata(["describe", operation])


class RunHandle:
    """Own one foreground CLI session and its job; explicit status/cancel/repair."""
    def __init__(self, project, request):
        self._lock = threading.Lock()
        self._process = subprocess.Popen(
            [_command(), "operations", "serve", "--project", str(project)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.job = None
        try:
            accepted = self._call({"method": "submit", "request": request})
            if "run_id" not in accepted:
                raise OperationError(accepted)
            self.job = accepted["run_id"]
        except BaseException:
            self.close()
            raise

    def _call(self, request):
        with self._lock:
            self._process.stdin.write(json.dumps(request).encode() + b"\n")
            self._process.stdin.flush()
            line = self._process.stdout.readline(_MAX_FRAME + 1)
            if not line or len(line) > _MAX_FRAME:
                raise OperationError({"errors": [{"code": "plugin_protocol_violation", "message": "Host job session closed or exceeded its frame limit"}]})
            return json.loads(line)

    def status(self):
        return self._call({"method": "status", "job": self.job})

    def cancel(self):
        return self._call({"method": "cancel", "job": self.job})

    def repair(self):
        return self._call({"method": "repair", "job": self.job})

    def result(self, timeout=None):
        """Wait for the owned result. A wait timeout does not cancel the job."""
        started = time.monotonic()
        while True:
            outcome = self.status()
            if outcome.get("state") in _TERMINAL:
                if outcome["state"] != "completed":
                    raise OperationError(outcome)
                return outcome
            if timeout is not None and time.monotonic() - started >= timeout:
                raise TimeoutError("Operation wait timed out; the handle still owns the job")
            time.sleep(0.025)

    def close(self):
        process = self._process
        if process.stdin and not process.stdin.closed:
            process.stdin.close()
        # EOF asks the host to cancel, kill/reap owned workers and flush receipts.
        process.wait()
        if process.stdout:
            process.stdout.close()
        if process.stderr:
            process.stderr.close()

    def __enter__(self):
        return self

    def __exit__(self, *_exception):
        self.close()


def submit(operation, *, project, inputs=None, parameters=None, destinations=None,
           pins=None, request_key=None):
    request = {"operation": operation, "inputs": {} if inputs is None else inputs, "parameters": {} if parameters is None else parameters,
               "destinations": {} if destinations is None else destinations}
    if pins is not None:
        request["pins"] = pins
    if request_key is not None:
        request["request_key"] = request_key
    return RunHandle(project, request)


def run(operation, **arguments):
    with submit(operation, **arguments) as handle:
        return handle.result()
