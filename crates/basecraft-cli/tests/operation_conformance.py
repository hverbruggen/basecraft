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


FAKE_TABLE2ASN = r"""#!/bin/sh
if [ "$1" = "-version" ]; then echo "table2asn: 0.0.test"; exit 0; fi
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "-outdir" ]; then out="$2"; shift; fi
  shift
done
id=$(grep -m1 '^>' submission.fsa | cut -c2- | cut -d' ' -f1)
echo "Seq-submit ::= {}" > "$out/submission.sqn"
printf 'Error: valid [SEQ_DESCR.BadGeoLocNameCode] Bad geo_loc_name BIOSEQ: lcl|%s: raw\n' "$id" > "$out/submission.val"
echo "LOCUS $id" > "$out/submission.gbf"
"""


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

    def mcp_name(operation):
        plugin, rest = operation.split("/")
        local, major = rest.split("@")
        return f"{plugin}.{local}.v{major}"

    def identity(outcome, publication):
        """Route-independent outcome and provenance content. Paths, run IDs and
        the recorded initiator legitimately differ between routes."""
        return {
            "operation": outcome["operation"], "state": outcome["state"],
            "pins": outcome["pins"], "data": outcome["data"],
            "requested": outcome["requested_parameters"],
            "effective": outcome["effective_parameters"],
            "artifacts": sorted((a["role"], a["digest"]) for a in outcome["artifacts"]),
            "inputs": sorted((i["logical_name"], i["hash"], i["size"]) for i in publication["inputs"]),
            "outputs": sorted((o["hash"], o["size"]) for o in publication["outputs"]),
            "completeness": publication["completeness"],
        }

    def run_request(path, value):
        path.write_text(json.dumps(value))
        outcome, _ = cli("operations", "run", value["operation"], "--project", str(project),
                         "--request", str(path), "--json")
        return outcome

    def python_error(**arguments):
        try:
            operations.run(**arguments)
        except operations.OperationError as error:
            return error.outcome
        raise AssertionError("Python accepted a request the host must refuse")

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
        routes = {"cli": (outcome, cli("provenance", "inspect", str(project / "cli.json"))[0])}
        with operations.submit(op, project=project, inputs=request("python.json")["inputs"],
                               parameters={"mode": "normal"},
                               destinations={"report": "python.json"}) as handle:
            python_outcome = handle.result(timeout=30)
            assert check_result(python_outcome, "python.json") == expected
        routes["python"] = (python_outcome,
                            cli("provenance", "inspect", str(project / "python.json"))[0])
        record("CLI/Python execute installed multi-input package with identical normalized data")

        mcp = Mcp(binary, project, env)
        try:
            tools = mcp.tools()
            # Discovery: the permitted CLI set and its schemas/pins are what MCP lists.
            listing, _ = cli("operations", "list", "--json")
            listed = {mcp_name(item["operation"]): item for item in listing["operations"]
                      if item["enabled"]}
            generated = {tool["name"]: tool for tool in tools if "basecraft" in tool.get("_meta", {})}
            assert set(generated) == set(listed), (set(generated) ^ set(listed))
            for tool_name, item in listed.items():
                tool = generated[tool_name]
                assert tool["inputSchema"]["properties"]["operation"] == {"const": item["operation"]}
                assert tool["inputSchema"]["properties"]["parameters"] == item["descriptor"]["parameters"]
                assert tool["_meta"]["basecraft"]["pins"] == item["pins"]
                assert tool["_meta"]["basecraft"]["origin"] == item["origin"]
                assert tool["description"] == item["descriptor"]["description"]
            record("CLI operations list and MCP tools/list agree on permitted operations, schemas and pins")
            metadata = next(tool for tool in tools if tool["name"] == name)
            assert metadata["inputSchema"]["properties"]["parameters"]["additionalProperties"] is False
            assert metadata["outputSchema"]["allOf"][1]["then"]["properties"]["data"]["required"] == ["bytes"]
            accepted = mcp.tool(name, request("mcp.json"))["structuredContent"]
            mcp_outcome = mcp.wait(accepted["run_id"])
            assert check_result(mcp_outcome, "mcp.json") == expected
            inspected = mcp.tool("provenance_inspect", {"path": "mcp.json"})
            assert not inspected["isError"], inspected
            routes["mcp"] = (mcp_outcome, inspected["structuredContent"])
            identities = {route: identity(*value) for route, value in routes.items()}
            assert identities["cli"] == identities["python"] == identities["mcp"], identities
            assert identities["cli"]["inputs"] and identities["cli"]["outputs"]
            for route, (outcome, publication) in routes.items():
                initiator = {"cli": "CLI", "python": "Python", "mcp": "mcp-client"}[route]
                assert publication["origin"].endswith("initiated by " + initiator), publication
            verified = mcp.tool("provenance_verify", {"path": "mcp.json"})["structuredContent"]
            assert verified == {"verified": True, "issues": []}, verified
            record("CLI/Python/MCP outcomes agree: pins, parameters, data, artifact digests, input hashes and provenance")
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
            invalid_corpus = []
            for parameters in ({"mode": "normal", "unknown_setting": True},
                               {"mode": "not-a-mode"}, {"include_newlines": "yes"}):
                invalid = request("invalid.json")
                invalid["parameters"] = parameters
                rejected = run_request(request_path, invalid)
                assert rejected["state"] == "failed", rejected
                assert rejected["errors"][0]["code"] == "invalid_arguments", rejected
                assert python_error(operation=op, project=project, inputs=invalid["inputs"],
                                    parameters=parameters,
                                    destinations=invalid["destinations"]) == rejected
                via_mcp = mcp.tool(name, invalid)
                assert via_mcp["isError"] and via_mcp["structuredContent"] == rejected, via_mcp
                invalid_corpus.append(rejected["errors"][0]["message"])
            assert len(set(invalid_corpus)) == 3, invalid_corpus
            assert not (project / "invalid.json").exists()
            record("invalid-parameter corpus: identical structured errors from CLI/Python/MCP")

            missing = "org.example.registry-fixture/missing@1"
            unknown = request("missing.json", operation=missing)
            rejected = run_request(request_path, unknown)
            assert rejected["errors"][0]["code"] == "plugin_unavailable", rejected
            # Compare Python by code: an MCP-based client may phrase its own message
            # until it surfaces the host's error.data.
            via_python = python_error(operation=missing, project=project, inputs=unknown["inputs"],
                                      parameters=unknown["parameters"],
                                      destinations=unknown["destinations"])
            assert via_python["errors"][0]["code"] == "plugin_unavailable", via_python
            error = mcp.call("tools/call", {"name": mcp_name(missing), "arguments": unknown},
                             error=True)
            assert error["code"] == -32602 and error["data"] == rejected, error
            assert not (project / "missing.json").exists()
            record("unknown operation: identical structured error from CLI and MCP error data; same Python code")

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

        # Python drives the shared support import over MCP; its file report is
        # the CLI history report (revision IDs name the reading session).
        from basecraft.session import Session
        (project / "target.nwk").write_text("((A:1,B:1)90:1,(C:1,D:1)80:1);")
        (project / "support.nwk").write_text("((A:1,B:1)75:1,(C:1,D:1)60:1);")
        with Session(project, writes=True) as session:
            tree = session.open("target.nwk")["document"]
            imported = session.import_support(tree, "support.nwk", "Bootstrap")
            assert imported["added"] == ["Bootstrap"] and imported["matched"] == 2, imported
            session.save(tree)
            python_record = session.file_record("target.nwk")
        cli_record, _ = cli("history", str(project / "target.nwk"), "--json")
        for report in (python_record, cli_record):
            assert any(row["summary"] == "Ran basecraft.tree import_support"
                       for row in report["history"]), report["history"]
        assert ([row["summary"] for row in python_record["history"]]
                == [row["summary"] for row in cli_record["history"]])
        assert python_record["methods"] == cli_record["methods"]
        subprocess.run([str(binary), "provenance", "verify", str(project / "target.nwk")],
                       cwd=root, env=env, capture_output=True, timeout=45, check=True)
        record("Python support import over MCP; file_record matches CLI history")

        readonly = Mcp(binary, project, env, writable=False)
        try:
            assert not any(tool["name"] == name for tool in readonly.tools())
            denied = readonly.tool(name, request("denied.json"))
            assert denied["isError"], denied
            denial = denied["structuredContent"]
            assert denial["state"] == "failed" and denial["operation"] == op, denial
            assert [e["code"] for e in denial["errors"]] == ["permission_denied"], denial
            assert "run_id" not in denial or denial["run_id"] is None
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
        assert denied["errors"][0]["code"] == "plugin_disabled", denied
        disabled_request = request("disabled.json")
        assert python_error(operation=op, project=project, inputs=disabled_request["inputs"],
                            parameters=disabled_request["parameters"],
                            destinations=disabled_request["destinations"]) == denied
        record("read-only denial and explicit disable survives update")

        # Project specimen metadata: the CLI, MCP and Python routes share one
        # planner and one typed edit, so they produce the same tables.
        fasta = ">HV1|Caulerpa_taxifolia|2019-05\nACGT\n>HV2|Caulerpa_sp._3|not collected\nACGA\n"
        sheet = "specimen_id,country,decimalLatitude\nHV1,Australia,-38.1\nHV1,Fiji,1\nHV2,Fiji,95\n"
        fields = ["specimen", "taxon", "eventDate"]
        projects = {}
        for route in ("cli", "mcp", "python"):
            projects[route] = root / f"metadata-{route}"
            projects[route].mkdir()
            (projects[route] / "rbcL.fasta").write_text(fasta)
            (projects[route] / "sheet.csv").write_text(sheet)
        plans = {}
        meta_project = projects["cli"]
        subprocess.run([str(binary), "metadata", "init", "--project", str(meta_project)],
                       env=env, check=True, capture_output=True, timeout=45)
        headers_plan, _ = cli("metadata", "import-headers", str(meta_project / "rbcL.fasta"),
                              "--separator", "|", "--fields", ",".join(fields),
                              "--project", str(meta_project), "--apply")
        sheet_plan, _ = cli("metadata", "import-sheet", str(meta_project / "sheet.csv"),
                            "--project", str(meta_project), "--apply")
        plans["cli"] = (headers_plan, sheet_plan)
        mcp_meta = Mcp(binary, projects["mcp"], env)
        try:
            opened = mcp_meta.tool("metadata_open", {"create": True})["structuredContent"]
            doc = opened["document"]
            pattern = {"name": "CLI", "separator": "|", "fields": [
                {"role": "specimen"}, {"role": "taxon"}, {"role": "column", "column": "eventDate"}]}
            first = mcp_meta.tool("metadata_plan_headers", {"document": doc, "request": {
                "document": "rbcL.fasta", "pattern": pattern}})["structuredContent"]
            state = mcp_meta.tool("state", {"document": doc})["structuredContent"]
            mcp_meta.tool("apply", {"document": doc, "version": state["version"], "operation": {
                "op": "metadata.edit", "params": {"edit": {"action": "apply",
                                                           "changes": first["changes"]}}}})
            second = mcp_meta.tool("metadata_plan_sheet", {"document": doc, "request": {
                "path": "sheet.csv"}})["structuredContent"]
            state = mcp_meta.tool("state", {"document": doc})["structuredContent"]
            mcp_meta.tool("apply", {"document": doc, "version": state["version"], "operation": {
                "op": "metadata.edit", "params": {"edit": {"action": "apply",
                                                           "changes": second["changes"]}}}})
            mcp_meta.tool("save", {"document": doc})
            plans["mcp"] = (first, second)
        finally:
            mcp_meta.close()
        from basecraft.session import Session as PythonSession
        with PythonSession(projects["python"], writes=True, command=str(binary)) as py:
            doc = py.open_metadata(create=True)["document"]
            first = py.plan_header_import(doc, "rbcL.fasta", "|", fields)
            py.apply_metadata(doc, first["changes"])
            second = py.plan_sheet_import(doc, "sheet.csv")
            py.apply_metadata(doc, second["changes"])
            py.save(doc)
            insdc = py.insdc_source(doc, "HV1")
            plans["python"] = (first, second)
        assert insdc["qualifiers"]["geo_loc_name"] == "Australia", insdc
        assert insdc["qualifiers"]["collection_date"] == "2019-05", insdc

        def comparable(plan):
            changes = copy.deepcopy(plan["changes"])
            taxa = {t["id"]: t["name"] for t in changes.pop("taxa", [])}
            identifications = sorted((i["specimen"], taxa.get(i["taxon"], i["taxon"]))
                                     for i in changes.pop("identifications", []))
            return changes, sorted(taxa.values()), identifications, plan["issues"]

        for route in ("mcp", "python"):
            for ours, theirs in zip(plans["cli"], plans[route]):
                assert comparable(ours) == comparable(theirs), (route, ours, theirs)
            for table in ("specimens.tsv", "records.tsv"):
                assert (projects["cli"] / "metadata" / table).read_bytes() == \
                    (projects[route] / "metadata" / table).read_bytes(), (route, table)
        kinds = sorted({i["kind"] for i in plans["cli"][1]["issues"]})
        assert kinds == ["duplicate", "invalid"], kinds
        assert "HV2\tnot collected" in (projects["cli"] / "metadata/specimens.tsv").read_text()
        status, _ = cli("metadata", "status", str(meta_project / "rbcL.fasta"),
                        "--project", str(meta_project))
        assert status["unlinked"] == [] and status["records"] == 2, status
        # The .ods template is a valid ZIP/OpenDocument file for other readers.
        import zipfile
        template = root / "template.ods"
        subprocess.run([str(binary), "metadata", "template", str(meta_project / "rbcL.fasta"),
                        "--output", str(template), "--project", str(meta_project)],
                       env=env, check=True, capture_output=True, timeout=45)
        with zipfile.ZipFile(template) as archive:
            assert archive.testzip() is None
            assert archive.namelist()[0] == "mimetype"
            assert archive.read("mimetype") == b"application/vnd.oasis.opendocument.spreadsheet"
            assert b"HV2|Caulerpa_sp._3|not" in archive.read("content.xml")
        record("specimen metadata imports agree across CLI, MCP and Python")

        # Consumers reuse those exact saved links and generic typed apply/save.
        consumer_results = {}
        workspace_fixture = Path(__file__).resolve().parents[3] / "testdata/fixtures/delimitation/manual.spart"
        for route, project in projects.items():
            shutil.copyfile(workspace_fixture, project / "result.spart")
            link_request = {"action": "link_workspace", "workspace": "result.spart",
                            "links": {"a": "HV1", "b": "HV2", "c": "HV1", "d": "HV2"}}
            promote = {"action": "promote_partition", "workspace": "result.spart",
                       "hypothesis": "partition-1", "id": "project-result"}
            labels = {"action": "labels", "document": "rbcL.fasta",
                      "records": ["HV1|Caulerpa_taxifolia|2019-05", "unlinked"],
                      "fields": ["genus", "species", "specimen", "accession"]}
            if route == "cli":
                for request in (link_request, promote):
                    cli("metadata", "consumer", json.dumps(request), "--project", str(project), "--apply")
                partition, _ = cli("metadata", "consumer", '{"action":"partitions"}', "--project", str(project))
                label_result, _ = cli("metadata", "consumer", json.dumps(labels), "--project", str(project))
            elif route == "python":
                with PythonSession(project, writes=True, command=str(binary)) as py:
                    meta = py.open_metadata()["document"]
                    for request in (link_request, promote):
                        plan = py.metadata_consumer(meta, request)
                        target = meta if request == promote else py.open(plan["target"])["document"]
                        py.apply(target, py.state(target)["version"], plan["operation"])
                        py.save(target)
                    partition = py.metadata_consumer(meta, {"action": "partitions"})
                    label_result = py.metadata_consumer(meta, labels)
            else:
                connection = Mcp(binary, project, env)
                try:
                    meta = connection.tool("metadata_open", {})["structuredContent"]["document"]
                    for request in (link_request, promote):
                        plan = connection.tool("metadata_consumer", {"document": meta, "request": request})["structuredContent"]
                        target = meta if request == promote else connection.tool("open", {"path": plan["target"]})["structuredContent"]["document"]
                        version = connection.tool("state", {"document": target})["structuredContent"]["version"]
                        connection.tool("apply", {"document": target, "version": version, "operation": plan["operation"]})
                        connection.tool("save", {"document": target})
                    partition = connection.tool("metadata_consumer", {"document": meta, "request": {"action": "partitions"}})["structuredContent"]
                    label_result = connection.tool("metadata_consumer", {"document": meta, "request": labels})["structuredContent"]
                finally:
                    connection.close()
            saved = next(p for p in partition["partitions"] if p["id"] == "project-result")
            assert sorted(sorted(g["members"]) for g in saved["groups"]) == [["HV1"], ["HV2"]]
            assert saved["provenance"]["inputs"]["project_metadata"]
            assert saved["provenance"]["parameters"]["source_hypothesis"]["id"] == "partition-1"
            assert label_result["labels"][0]["label"] == "HV1"
            assert label_result["labels"][1]["issue"]
            consumer_results[route] = label_result["labels"]
        assert consumer_results["cli"] == consumer_results["mcp"] == consumer_results["python"]
        record("specimen partitions, explicit workspace links and display labels agree across CLI, MCP and Python")

        # GenBank marker submissions: the CLI, MCP and Python routes share one
        # review, one package writer and the registered table2asn operation,
        # run here against a stand-in table2asn.
        checkout = Path(__file__).resolve().parents[3]
        bundle = root / "table2asn-bundle"
        shutil.copytree(checkout / "plugins/table2asn", bundle / "table2asn")
        fake = root / "fake-table2asn"
        fake.write_text(FAKE_TABLE2ASN)
        fake.chmod(0o755)
        sub_env = dict(env, BASECRAFT_PLUGINS=str(bundle), BASECRAFT_TABLE2ASN=str(fake))
        saved_environment = dict(os.environ)
        os.environ.update(sub_env)
        try:
            def sub_cli(*arguments):
                completed = subprocess.run([str(binary), *arguments], cwd=root, env=sub_env,
                                           capture_output=True, text=True, timeout=90, check=True)
                return json.loads(completed.stdout)

            person = {"first_name": "Jane", "last_name": "Doe", "email": "jane@example.org",
                      "institution": "University of Melbourne", "city": "Melbourne",
                      "country": "Australia"}
            assert sub_cli("submission", "submitter", "--set", json.dumps(person))["missing"] == []
            edits = [
                {"action": "set_batch", "batch": {"marker": {"kind": "its", "components": [
                    "SSU", "ITS1", "5.8S", "ITS2"]}, "genetic_code": 1}},
                {"action": "set_publication", "publication": {"title": "ITS markers", "authors": [
                    {"first": "Jane", "last": "Doe"}]}},
                {"action": "set_rows", "rows": [{"record": "orphan_ITS", "decision": "exclude"}]},
            ]
            projects = {}
            for route in ("cli", "mcp", "python"):
                projects[route] = root / f"submission-{route}"
                projects[route].mkdir()
                for name in ("ITS.afa", "specimens.csv"):
                    shutil.copy(checkout / "testdata/fixtures/submission" / name, projects[route])
                subprocess.run([str(binary), "metadata", "init", "--project", str(projects[route])],
                               env=sub_env, check=True, capture_output=True, timeout=45)
                sub_cli("metadata", "import-sheet", str(projects[route] / "specimens.csv"),
                        "--project", str(projects[route]), "--apply")
            reviews, summaries = {}, {}
            cli_project = projects["cli"]
            document = str(cli_project / "its.submission.json")
            sub_cli("submission", "new", document, "--alignment", str(cli_project / "ITS.afa"),
                    "--project", str(cli_project))
            for edit in edits:
                sub_cli("submission", "edit", document, json.dumps(edit), "--project", str(cli_project))
            reviews["cli"] = sub_cli("submission", "review", document, "--project", str(cli_project))
            refused = subprocess.run([str(binary), "submission", "export", document,
                                      "--output", str(cli_project / "unconfirmed"),
                                      "--project", str(cli_project)], cwd=root, env=sub_env,
                                     capture_output=True, text=True, timeout=90)
            assert refused.returncode != 0 and "confirm" in refused.stderr.lower(), refused.stderr
            assert not (cli_project / "unconfirmed").exists()
            sub_cli("submission", "edit", document, json.dumps({
                "action": "confirm_selection", "selection_hash": reviews["cli"]["selection_hash"]}),
                "--project", str(cli_project))
            sub_cli("submission", "export", document, "--output", str(cli_project / "package"),
                    "--project", str(cli_project))
            summaries["cli"] = sub_cli("submission", "validate", str(cli_project / "package"),
                                       "--project", str(cli_project))
            mcp_sub = Mcp(binary, projects["mcp"], sub_env)
            try:
                created = mcp_sub.tool("submission_create", {"path": "its.submission.json",
                                                             "alignment": "ITS.afa"})["structuredContent"]
                doc = created["document"]
                assert created["kind"] == "submission", created
                for edit in edits:
                    state = mcp_sub.tool("state", {"document": doc})["structuredContent"]
                    mcp_sub.tool("apply", {"document": doc, "version": state["version"], "operation": {
                        "op": "submission.edit", "params": {"edit": edit}}})
                mcp_sub.tool("save", {"document": doc})
                reviews["mcp"] = mcp_sub.tool("submission_review", {"document": doc})["structuredContent"]
                state = mcp_sub.tool("state", {"document": doc})["structuredContent"]
                mcp_sub.tool("apply", {"document": doc, "version": state["version"], "operation": {
                    "op": "submission.edit", "params": {"edit": {
                        "action": "confirm_selection", "selection_hash": reviews["mcp"]["selection_hash"]}}}})
                mcp_sub.tool("submission_export", {"document": doc, "output": "package"})
                validation = mcp_sub.tool("submission_validation_request",
                                          {"package": "package"})["structuredContent"]
                accepted = mcp_sub.tool("basecraft.table2asn.validate.v1", validation)["structuredContent"]
                mcp_sub.wait(accepted["run_id"])
                outcome = mcp_sub.tool("basecraft.host.jobs_result.v1",
                                       {"job": accepted["run_id"]})["structuredContent"]
                summaries["mcp"] = mcp_sub.tool("submission_record_validation", {
                    "package": "package", "outcome": outcome})["structuredContent"]
            finally:
                mcp_sub.close()
            with PythonSession(projects["python"], writes=True, command=str(binary)) as py:
                doc = py.create_submission("its.submission.json", "ITS.afa")["document"]
                for edit in edits:
                    py.edit_submission(doc, edit)
                py.save(doc)
                reviews["python"] = py.review_submission(doc)
                py.edit_submission(doc, {"action": "confirm_selection",
                                         "selection_hash": reviews["python"]["selection_hash"]})
                py.export_submission(doc, "package")
                summaries["python"] = py.validate_submission("package", timeout=60)
            # Each project's metadata manifest has its own generated IDs.
            for review in reviews.values():
                review.pop("metadata_hash")
                # The fingerprint incorporates each project's metadata identity.
                review.pop("selection_hash")
            codes = sorted({(i["code"], i.get("record")) for i in reviews["cli"]["issues"]})
            assert ("duplicate_record", "HV403_ITS") in codes, codes
            for route in ("mcp", "python"):
                if reviews[route] != reviews["cli"]:
                    def first_difference(a, b, where="review"):
                        if type(a) is not type(b) or not isinstance(a, (dict, list)):
                            return None if a == b else (where, a, b)
                        keys = sorted(set(a) | set(b)) if isinstance(a, dict) else range(max(len(a), len(b)))
                        for key in keys:
                            try:
                                found = first_difference(a[key], b[key], f"{where}/{key}")
                            except (KeyError, IndexError):
                                return (f"{where}/{key}", "missing on one side", None)
                            if found:
                                return found
                        return None
                    raise AssertionError((route, first_difference(reviews["cli"], reviews[route])))
                for name in ("genbank/submission.fsa", "genbank/submission.src",
                             "genbank/submission.tbl", "genbank/submission.sbt",
                             "genbank/submission.sqn", "records.tsv", "specimens.tsv",
                             "coordinate-map.tsv"):
                    assert (projects["cli"] / "package" / name).read_bytes() == \
                        (projects[route] / "package" / name).read_bytes(), (route, name)
            for route, summary in summaries.items():
                assert summary["status"] == "errors" and summary["tool_version"] == "0.0.test", (route, summary)
                assert summary["issues"][0]["record"] == "HV401_ITS", (route, summary)
                assert summary["issues"][0]["specimen"] == "HV401", (route, summary)
        finally:
            os.environ.clear()
            os.environ.update(saved_environment)
        record("GenBank submissions agree across CLI, MCP and Python")
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == receipt["sha256"]
        receipt["status"] = "passed"
    finally:
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
    print(receipt_path)


if __name__ == "__main__":
    main()
