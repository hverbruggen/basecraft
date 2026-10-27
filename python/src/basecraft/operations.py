"""Installed operations through the shared host, using independent headless sessions.

Schemas, defaults, permissions, execution and provenance belong to the host.
``submit`` and ``run`` start their own writable MCP session (``basecraft mcp
--allow-writes``), matching ``basecraft operations run``. Closing a handle
cancels its unfinished job and ends that session; it never attaches to a GUI.
Use ``basecraft.session.Session`` to run several operations and document edits
in one session.
"""
from __future__ import annotations

import json
import subprocess

from .session import Job, OperationError, Session
from .tools import executable

__all__ = ["OperationError", "RunHandle", "describe", "list", "run", "submit"]


def _metadata(arguments):
    result = subprocess.run([executable(["basecraft"], "BASECRAFT"), "operations", *arguments,
                             "--json"], capture_output=True, text=True, check=False)
    if result.returncode:
        raise OperationError({"errors": [{"code": "plugin_unavailable",
                                          "message": result.stderr.strip()}]})
    return json.loads(result.stdout)


def list():
    """Return the current registry generation, descriptors and diagnostics."""
    return _metadata(["list"])


def describe(operation):
    return _metadata(["describe", operation])


class RunHandle(Job):
    """One job in its own writable session; explicit status/cancel/repair/result."""

    def __init__(self, project, operation, **arguments):
        session = Session(project, writes=True)
        try:
            job = session.submit(operation, **arguments)
        except BaseException:
            session.close()
            raise
        super().__init__(session, job.accepted)

    def close(self):
        self.session.close()

    def __enter__(self):
        return self

    def __exit__(self, *_exception):
        self.close()


def submit(operation, *, project, inputs=None, parameters=None, destinations=None,
           pins=None, request_key=None):
    return RunHandle(project, operation, inputs=inputs, parameters=parameters,
                     destinations=destinations, pins=pins, request_key=request_key)


def run(operation, **arguments):
    with submit(operation, **arguments) as handle:
        return handle.result()
