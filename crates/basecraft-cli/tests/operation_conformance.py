"""Acceptance against an unchanged built CLI; does not compile anything.

Run with the matching SDK Python (pyarrow required):
  PYTHONPATH=python/src python operation_conformance.py --binary /abs/basecraft \
      --root /external/disposable-directory
The root must not exist. All profiles, package updates and results stay there.
Failures retain the root and partial receipt for diagnosis.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import sys
import time


class Mcp:
    def __init__(self, binary, project, env, writable=True):
        command = [str(binary), "mcp", str(project)]
        if writable:
            command.append("--allow-writes")
        self.process = subprocess.Popen(command, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.serial = 0
        self.notifications = []
        self.pending = bytearray()
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.call("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                 "clientInfo": {"name": "conformance", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, value):
        self.process.stdin.write(json.dumps(value).encode() + b"\n")
        self.process.stdin.flush()

    def read(self, deadline):
        while b"\n" not in self.pending:
            remaining = deadline - time.monotonic()
            assert remaining > 0 and self.selector.select(remaining), "MCP response timeout"
            chunk = os.read(self.process.stdout.fileno(), 65536)
            assert chunk, "MCP disconnected"
            self.pending.extend(chunk)
            assert len(self.pending) <= 8 * 1024 * 1024, "MCP frame too large"
        line, _, tail = self.pending.partition(b"\n")
        self.pending = bytearray(tail)
        return json.loads(line)

    def call(self, method, params=None, error=False):
        self.serial += 1
        self.send({"jsonrpc": "2.0", "id": self.serial, "method": method,
                   "params": {} if params is None else params})
        deadline = time.monotonic() + 10
        while True:
            reply = self.read(deadline)
            if "id" not in reply:
                self.notifications.append(reply)
                continue
            assert reply["id"] == self.serial, reply
            if error:
                assert "error" in reply, reply
                return reply["error"]
            assert "error" not in reply, reply
            return reply["result"]

    def tool(self, name, arguments):
        return self.call("tools/call", {"name": name, "arguments": arguments})

    def tools(self):
        result, cursor = [], None
        while True:
            page = self.call("tools/list", {} if cursor is None else {"cursor": cursor})
            result.extend(page["tools"])
            cursor = page.get("nextCursor")
            if cursor is None:
                return result

    def wait(self, job):
        deadline = time.monotonic() + 30
        while True:
            outcome = self.tool("basecraft.host.jobs_get.v1", {"job": job})["structuredContent"]
            if outcome.get("state") in {"completed", "failed", "cancelled", "partial", "interrupted"}:
                return outcome
            assert time.monotonic() < deadline, outcome
            time.sleep(0.025)

    def close(self):
        self.process.stdin.close()
        try:
            assert self.process.wait(timeout=25) == 0
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait()
            self.selector.close()
            self.process.stdout.close()
            self.process.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = args.root.absolute()
    root.mkdir()  # Refuse reuse, including the production/shared validation profile.
    receipt = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "checks": [], "status": "incomplete"}
    receipt_path = root / "receipt.json"

    def record(name):
        receipt["checks"].append(name)
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")

    fixture = Path(__file__).resolve().parents[3] / "testdata/fixtures/registry-plugin"
    package = root / "external-package"
    shutil.copytree(fixture, package)
    # The isolated profile deliberately has no bundled tools. Supply enough
    # declared aliases to exercise multiple MCP pages independently of bundles.
    contract_path = package / "operations.json"
    contract = json.loads(contract_path.read_text())
    for index in range(20):
        alias = copy.deepcopy(contract["operations"][0])
        alias["name"] = f"page_fixture_{index}"
        contract["operations"].append(alias)
    contract_path.write_text(json.dumps(contract))
    project = root / "project"
    project.mkdir()
    (project / "a.txt").write_bytes(b"abc\n")
    (project / "b.txt").write_bytes(b"xy\n")
    sentinel = root / "import-sentinel"
    env = dict(os.environ, BASECRAFT=str(binary), BASECRAFT_PROFILE_DIR=str(root / "profile"),
               BASECRAFT_PLUGINS=str(root / "empty-bundle"), BASECRAFT_PYTHON=sys.executable,
               BASECRAFT_FIXTURE_SENTINEL=str(sentinel))
    # The host adds its matching SDK to plugin PYTHONPATH. This process imports
    # the SDK selected by the caller; do not silently import another checkout.
    os.environ.update(env)
    from basecraft import operations

    def cli(*arguments):
        completed = subprocess.run([str(binary), *arguments], cwd=root, env=env,
                                   capture_output=True, text=True, timeout=45, check=True)
        return json.loads(completed.stdout), completed.stderr

    op = "org.example.registry-fixture/count@1"
    name = "org.example.registry-fixture.count.v1"

    def request(output, mode="normal", operation=op):
        return {"operation": operation, "parameters": {"mode": mode},
                "inputs": {"sources": [{"kind": "file", "path": "a.txt"},
                                       {"kind": "file", "path": "b.txt"}]},
                "destinations": {"report": output}}

    def check_result(outcome, output):
        assert outcome["state"] == "completed", outcome
        assert outcome["data"] == {"bytes": 7}, outcome
        assert outcome["effective_parameters"] == {"mode": "normal", "include_newlines": True}
        assert json.loads((project / output).read_text()) == {"bytes": 7}
        assert Path(outcome["evidence"]).is_dir(), outcome
        return outcome["effective_parameters"], outcome["data"]

    try:
        _, warning = cli("plugins", "install", str(package))
        assert "user account" in warning and "responsib" in warning
        listing, _ = cli("operations", "list", "--json")
        assert any(item["operation"] == op and item["enabled"] for item in listing["operations"])
        assert operations.describe(op)["descriptor"]["result"]["required"] == ["bytes"]
        assert not sentinel.exists(), "discovery executed entry script"
        record("install auto-enables; warning stderr; CLI/Python discovery is passive")

        request_path = root / "request.json"
        request_path.write_text(json.dumps(request("cli.json")))
        outcome, _ = cli("operations", "run", op, "--project", str(project),
                         "--request", str(request_path), "--json")
        expected = check_result(outcome, "cli.json")
        with operations.submit(op, project=project, inputs=request("python.json")["inputs"],
                               parameters={"mode": "normal"},
                               destinations={"report": "python.json"}) as handle:
            assert check_result(handle.result(timeout=30), "python.json") == expected
        record("CLI/Python execute installed multi-input package with identical normalized data")

        mcp = Mcp(binary, project, env)
        try:
            tools = mcp.tools()
            metadata = next(tool for tool in tools if tool["name"] == name)
            assert metadata["inputSchema"]["properties"]["parameters"]["additionalProperties"] is False
            assert metadata["outputSchema"]["allOf"][1]["then"]["properties"]["data"]["required"] == ["bytes"]
            accepted = mcp.tool(name, request("mcp.json"))["structuredContent"]
            assert check_result(mcp.wait(accepted["run_id"]), "mcp.json") == expected
            unknown = mcp.tool("basecraft.host.jobs_get.v1", {"job": "guessed-job-id"})
            assert unknown["isError"]
            slow = mcp.tool(name, request("slow.json", "slow"))["structuredContent"]
            start = time.monotonic()
            assert mcp.call("ping") == {}
            assert time.monotonic() - start < 2
            mcp.tool("basecraft.host.jobs_cancel.v1", {"job": slow["run_id"]})
            assert mcp.wait(slow["run_id"])["state"] == "cancelled"
            assert not (project / "slow.json").exists()
            record("MCP parity, declared schemas, scoped IDs, responsive ping and cancellation")
            invalid = request("invalid.json")
            invalid["parameters"]["unknown_setting"] = True
            request_path.write_text(json.dumps(invalid))
            rejected, _ = cli("operations", "run", op, "--project", str(project),
                              "--request", str(request_path), "--json")
            assert rejected["state"] == "failed"
            try:
                operations.run(op, project=project, inputs=invalid["inputs"],
                               parameters=invalid["parameters"], destinations=invalid["destinations"])
            except operations.OperationError:
                pass
            else:
                raise AssertionError("Python accepted unknown scientific setting")
            assert mcp.tool(name, invalid)["isError"]
            assert not (project / "invalid.json").exists()
            record("same invalid-parameter corpus rejected by CLI/Python/MCP")

            first_page = mcp.call("tools/list")
            old_cursor = first_page["nextCursor"]
            # A compatible package update only: the host binary is untouched.
            contract = json.loads((package / "operations.json").read_text())
            second = copy.deepcopy(contract["operations"][0])
            second.update(name="count_again", implementation="count_again")
            contract["operations"].append(second)
            (package / "operations.json").write_text(json.dumps(contract))
            script = package / "plugin.py"
            script.write_text(script.read_text().replace('if __name__ == "__main__":',
                              'plugin.context_operation("count_again")(count)\n\nif __name__ == "__main__":'))
            cli("plugins", "install", str(package))
            mcp.tool("basecraft.host.registry_refresh.v1", {})
            assert mcp.call("tools/list", {"cursor": old_cursor}, error=True)["code"] == -32602
            second_name = "org.example.registry-fixture.count_again.v1"
            assert any(tool["name"] == second_name for tool in mcp.tools())
            assert any(note["method"] == "notifications/tools/list_changed" for note in mcp.notifications)
            second_op = "org.example.registry-fixture/count_again@1"
            accepted = mcp.tool(second_name, request("second.json", operation=second_op))["structuredContent"]
            check_result(mcp.wait(accepted["run_id"]), "second.json")
            assert operations.describe(second_op)["operation"] == second_op
            request_path.write_text(json.dumps(request("second-cli.json", operation=second_op)))
            second_cli, _ = cli("operations", "run", second_op, "--project", str(project),
                                "--request", str(request_path), "--json")
            check_result(second_cli, "second-cli.json")
            second_python = operations.run(second_op, project=project,
                inputs=request("second-python.json")["inputs"], parameters={"mode":"normal"},
                destinations={"report":"second-python.json"})
            check_result(second_python, "second-python.json")
            record("installed compatible operation update, refresh notification and invocation without rebuild")
        finally:
            mcp.close()

        # Disconnect asks the owning manager to cancel its outstanding process.
        disconnected = Mcp(binary, project, env)
        pending = disconnected.tool(name, request("disconnect.json", "hang"))["structuredContent"]
        assert pending["run_id"]
        disconnected.close()
        assert not (project / "disconnect.json").exists()
        record("MCP EOF cancels outstanding owned operation")

        readonly = Mcp(binary, project, env, writable=False)
        try:
            assert not any(tool["name"] == name for tool in readonly.tools())
            assert readonly.tool(name, request("denied.json"))["isError"]
            assert not (project / "denied.json").exists()
        finally:
            readonly.close()
        cli("plugins", "disable", "org.example.registry-fixture")
        cli("plugins", "install", str(package))
        disabled = operations.describe(op)
        assert not disabled["enabled"]
        request_path.write_text(json.dumps(request("disabled.json")))
        denied, _ = cli("operations", "run", op, "--project", str(project),
                        "--request", str(request_path), "--json")
        assert denied["state"] == "failed" and not (project / "disabled.json").exists()
        record("read-only denial and explicit disable survives update")
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == receipt["sha256"]
        receipt["status"] = "passed"
    finally:
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
    print(receipt_path)


if __name__ == "__main__":
    main()
