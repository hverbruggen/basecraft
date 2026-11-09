#!/usr/bin/env python3
"""Validate the packaged native plugin through an already built shared host.

Example: PYTHONPATH=python/src python3 plugins/sanderson/test_host_integration.py
  --binary /abs/basecraft --native-binary /abs/basecraft-sanderson-native
  --root /abs/disposable-new-directory
The root must not exist. No compilation or package publication occurs.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from native.tools.stage_package import stage


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--native-binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, help="Validate an already staged complete bundle")
    args = parser.parse_args()
    host = args.binary.resolve(strict=True)
    native = args.native_binary.resolve(strict=True)
    root = args.root.absolute()
    root.mkdir()
    bundled = args.bundle.resolve(strict=True) / "plugins" if args.bundle else root / "bundled"
    package = bundled / "sanderson"
    if args.bundle:
        staged = {"binary_sha256": hashlib.sha256(native.read_bytes()).hexdigest()}
        expected = json.loads((package / "bin/engine.json").read_text())
        assert staged["binary_sha256"] == expected["binary_sha256"]
    else:
        shutil.copytree(Path(__file__).resolve().parent, package,
                       ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "bin"))
        staged = stage(package, native)
    project = root / "project"
    project.mkdir()
    source = "(((t1:0.075,t2:0.075):0.05,(t3:0.075,t4:0.075):0.05):0.05,((t5:0.075,t6:0.075):0.05,(t7:0.075,t8:0.075):0.05):0.05);\n"
    (project / "tree.nwk").write_bytes(source.encode("utf-8"))
    source_hash = hashlib.sha256(source.encode()).hexdigest()
    env = dict(os.environ, BASECRAFT=str(host), BASECRAFT_PLUGINS=str(bundled),
               BASECRAFT_PROFILE_DIR=str(root / "profile"), BASECRAFT_PYTHON=sys.executable,
               BASECRAFT_PYTHONPATH=str(Path(__file__).resolve().parents[2] / "python/src"))
    os.environ.update(env)
    from basecraft import operations
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "crates/basecraft-cli/tests"))
    from operation_conformance import Mcp

    op = "basecraft.sanderson/date_tree@1"
    tool = "basecraft.sanderson.date_tree.v1"
    parameters = {"branch_units": "substitutions_per_site", "sites": 953,
                  "root_age": 1.0, "time_unit": "relative units", "lambda": 1.0,
                  "starts": 3, "seed": 17}
    receipt = {"host_binary_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
               "native": staged, "checks": [], "status": "incomplete"}
    receipt_path = root / "receipt.json"

    def record(name):
        receipt["checks"].append(name)
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")

    def cli(*arguments):
        result = subprocess.run([str(host), *arguments], cwd=root, env=env,
                                capture_output=True, text=True, timeout=90, check=False)
        if result.returncode:
            raise AssertionError((arguments, result.returncode, result.stdout, result.stderr))
        return json.loads(result.stdout)

    def request(prefix, params=parameters):
        return {"operation": op, "inputs": {"tree": [{"kind": "file", "path": "tree.nwk"}]},
                "parameters": params,
                "destinations": {"chronogram": prefix + ".nwk", "report": prefix + ".json"}}

    def check(outcome, prefix):
        assert outcome["state"] == "completed", outcome
        assert outcome["data"]["node_count"] == 15, outcome
        assert outcome["data"]["engine_sha256"] == staged["binary_sha256"], outcome
        report = json.loads((project / (prefix + ".json")).read_text())
        assert report["source_sha256"] == source_hash, report
        assert report["source_newick"] == source, report
        assert report["native_engine"]["binary_sha256"] == staged["binary_sha256"], report
        assert len(report["nodes"]) == 15, report
        assert (project / (prefix + ".nwk")).read_text().endswith(";\n")
        for name in (prefix + ".nwk", prefix + ".json"):
            verified = subprocess.run([str(host), "provenance", "verify", str(project / name)],
                                      env=env, capture_output=True, text=True, timeout=30, check=False)
            assert verified.returncode == 0, verified.stdout + verified.stderr
        return outcome["data"], report["result"]

    try:
        validation = cli("plugins", "validate", str(package))
        assert validation["schema"] == 2 and validation["id"] == "basecraft.sanderson"
        listing = cli("operations", "list", "--json")
        assert any(item["operation"] == op and item["enabled"] for item in listing["operations"]), listing
        descriptor = operations.describe(op)["descriptor"]
        assert descriptor["name"] == "date_tree" and descriptor["version"] == "1.0.0", descriptor
        record("bundled schema-2 validation and passive CLI/Python discovery")

        request_path = root / "request.json"
        request_path.write_text(json.dumps(request("cli")))
        cli_result = check(cli("operations", "run", op, "--project", str(project),
                               "--request", str(request_path), "--json"), "cli")
        python_result = check(operations.run(op, project=project, inputs=request("python")["inputs"],
                                             parameters=parameters,
                                             destinations=request("python")["destinations"]), "python")
        assert cli_result == python_result, (cli_result, python_result)
        record("CLI/Python shared runner and native result parity")

        mcp = Mcp(host, project, env)
        try:
            assert any(item["name"] == tool for item in mcp.tools())
            accepted = mcp.tool(tool, request("mcp"))["structuredContent"]
            mcp_result = check(mcp.wait(accepted["run_id"]), "mcp")
            assert mcp_result == cli_result, (mcp_result, cli_result)
        finally:
            mcp.close()
        record("headless MCP shared runner and native result parity")

        bounded = dict(parameters, constraints=[{"node": "N000002", "min_age": 0.5,
                                                 "max_age": 0.5}])
        bounded_result = operations.run(op, project=project,
                                        inputs=request("bounded")["inputs"],
                                        parameters=bounded,
                                        destinations=request("bounded")["destinations"])
        _, bound_fit = check(bounded_result, "bounded")
        assert abs(bound_fit["ages"][1] - 0.5) < 1e-8, bound_fit
        record("hard internal age bound carried through host and native engine")

        cv = dict(parameters)
        del cv["lambda"]
        cv["cross_validation"] = {"log10_start": -2.0, "log10_step": 2.0, "count": 2}
        cv_result = operations.run(op, project=project, inputs=request("cv")["inputs"],
                                   parameters=cv, destinations=request("cv")["destinations"])
        _, cv_fit = check(cv_result, "cv")
        assert len(cv_fit["cross_validation"]) == 2, cv_fit
        assert cv_fit["lambda"] in (0.01, 1.0), cv_fit
        record("explicit two-point terminal-branch CV through shared runner")

        bad = request("invalid")
        bad["parameters"] = dict(parameters, lambda_extra=2)
        request_path.write_text(json.dumps(bad))
        failed = cli("operations", "run", op, "--project", str(project),
                     "--request", str(request_path), "--json")
        assert failed["state"] == "failed", failed
        assert not (project / "invalid.nwk").exists()
        assert not (project / "invalid.json").exists()
        record("invalid parameter rejected without publishing artifacts")
        assert hashlib.sha256((project / "tree.nwk").read_bytes()).hexdigest() == source_hash
        record("source tree remains byte-identical")
        receipt["status"] = "passed"
    finally:
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
