"""Retain offline demo packages through CLI, Python and raw MCP submission APIs.

Requires an already built Basecraft CLI, its matching Python SDK and real native
table2asn. Does not build, download, upload or submit anything. The destination
must be new; failures retain partial evidence. See README.md for invocation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from basecraft.session import Session

CASES = {
    "COI": ({"kind": "coding", "gene": "COI", "product": "cytochrome c oxidase subunit I"},
            5, "mitochondrion", {"HV201_COI": 2, "HV202_COI": 2, "HV203_COI": 2},
            ["HV204_COI"], None),
    "18S": ({"kind": "rrna", "product": "18S ribosomal RNA"}, 1, None, {}, [], None),
    "rbcL": ({"kind": "coding", "gene": "rbcL", "product":
              "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit"},
             11, "chloroplast", {"HV101_rbcL": 2, "HV102_rbcL": 3, "HV103_rbcL": 2},
             ["REF_rbcL"], ("SEQ_FEAT.InternalStop", "HV103_rbcL")),
    "ITS": ({"kind": "its", "components": ["SSU", "ITS1", "5.8S", "ITS2"]},
            1, None, {}, ["orphan_ITS"], ("SEQ_DESCR.BadGeoLocNameCode", "HV404_ITS")),
}
DEMO = {"first_name": "Demo", "last_name": "Example", "email": "demo@example.invalid",
        "institution": "Basecraft demonstration only", "city": "Demo", "country": "Australia"}
NOTICE = """DEMONSTRATION ONLY — NOT A REAL STUDY OR AN ARCHIVE SUBMISSION

Sequences and specimen values are representative public/generated test fixtures;
the submitter and author are explicit placeholders, not real submitter details.
COI and 18S are expected locally valid examples. rbcL and ITS are intentional
negative controls (frameshift and invalid geography). Local validation does not
establish taxonomy, contamination clearance, ownership or GenBank acceptance.

For a real study, replace every fixture with the actual reviewed sequences and
specimen links, legal submitter/contact/address, authorized authors and title,
organism identifications, collection dates/locations, voucher repository and
catalogue identifiers, genetic code/organelle, marker and confirmed frames.
Review all inclusion/exclusion decisions and ambiguous bases, then export and
validate afresh. A changed input invalidates this evidence. Submit manually
through the archive's own workflow only after scientific and metadata review.
"""


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--table2asn", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    binary, tool, root = args.binary.resolve(strict=True), args.table2asn.resolve(strict=True), args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    (root / "DEMO-ONLY.txt").write_text(NOTICE)
    checkout = Path(__file__).resolve().parents[2]
    fixtures = checkout / "testdata/fixtures/submission"
    os.environ.update(BASECRAFT_PROFILE_DIR=str(root / "profile"),
                      BASECRAFT_PLUGINS=str(checkout / "plugins"),
                      BASECRAFT_TABLE2ASN=str(tool), BASECRAFT_PYTHON=sys.executable,
                      PYTHONDONTWRITEBYTECODE="1")
    receipt = {"status": "incomplete", "demo_only": True, "binary": str(binary),
               "binary_sha256": digest(binary), "qualifier_sha256": digest(Path(__file__)),
               "sdk_sha256": digest(Path(sys.modules[Session.__module__].__file__)),
               "python": sys.executable, "table2asn": str(tool),
               "table2asn_sha256": digest(tool), "cases": []}
    transcript = root / "commands.jsonl"

    def record(kind, request, response):
        with transcript.open("a") as output:
            output.write(json.dumps({"kind": kind, "request": request, "response": response}) + "\n")

    def cli(*arguments, refusal=False, json_result=True):
        command = [str(binary), *map(str, arguments)]
        done = subprocess.run(command, capture_output=True, text=True, timeout=180, check=False)
        record("cli", command, {"exit": done.returncode, "stdout": done.stdout, "stderr": done.stderr})
        if refusal:
            assert done.returncode != 0 and "confirm" in done.stderr.lower(), done.stderr
            return
        assert done.returncode == 0, (command, done.stderr)
        return json.loads(done.stdout) if json_result else done.stdout

    version = subprocess.run([str(tool), "-version"], capture_output=True, text=True,
                             timeout=60, check=True)
    receipt["table2asn_version_output"] = version.stdout + version.stderr
    assert "table2asn:" in receipt["table2asn_version_output"], receipt
    write_json(root / "receipt.json", receipt)
    cli("submission", "submitter", "--set", json.dumps(DEMO))
    try:
        for route in ("cli", "python", "mcp"):
            project = root / route
            project.mkdir()
            (project / "DEMO-ONLY.txt").write_text(NOTICE)
            for fixture in fixtures.iterdir():
                if fixture.suffix in (".afa", ".csv"):
                    shutil.copyfile(fixture, project / fixture.name)
            cli("metadata", "init", "--project", project, json_result=False)
            cli("metadata", "import-sheet", project / "specimens.csv", "--project", project, "--apply")
            for name, (marker, code, organelle, frames, excluded, negative) in CASES.items():
                document = f"{name}.submission.json"
                batch = {"marker": marker, "genetic_code": code}
                if organelle:
                    batch["organelle"] = organelle
                edits = [
                    {"action": "set_batch", "batch": batch},
                    {"action": "set_publication", "publication": {
                        "title": "DEMO ONLY: representative marker qualification", "authors": [
                            {"first": "Demo", "last": "Example"}]}},
                    {"action": "set_rows", "rows": [
                        {"record": row, "decision": "exclude"} for row in excluded]},
                    {"action": "set_frames", "frames": [
                        {"record": row, "codon_start": frame} for row, frame in frames.items()]},
                ]
                if route == "cli":
                    path = project / document
                    cli("submission", "new", path, "--alignment", project / f"{name}.afa", "--project", project)
                    for edit in edits:
                        cli("submission", "edit", path, json.dumps(edit), "--project", project)
                    review = cli("submission", "review", path, "--project", project)
                    cli("submission", "export", path, "--output", project / f"{name}-unconfirmed",
                        "--project", project, refusal=True)
                    cli("submission", "edit", path, json.dumps({"action": "confirm_selection",
                        "selection_hash": review["selection_hash"]}), "--project", project)
                    # Every CLI command opens the saved document in a fresh session.
                    reopened = cli("submission", "review", path, "--project", project)
                    cli("submission", "export", path, "--output", project / name, "--project", project)
                    summary = cli("submission", "validate", project / name, "--project", project)
                else:
                    with Session(project, writes=True, command=str(binary)) as session:
                        def call(method, parameters):
                            result = session.call(method, parameters)
                            record("mcp", {"tool": method, "arguments": parameters}, result)
                            return result

                        doc = (session.create_submission(document, f"{name}.afa") if route == "python"
                               else call("submission_create", {"path": document, "alignment": f"{name}.afa"}))["document"]

                        def edit_doc(edit, route=route, doc=doc):
                            if route == "python":
                                session.edit_submission(doc, edit)
                            else:
                                call("apply", {"document": doc, "version": session.state(doc)["version"],
                                     "operation": {"op": "submission.edit", "params": {"edit": edit}}})

                        for edit in edits:
                            edit_doc(edit)
                        review = (session.review_submission(doc) if route == "python" else
                                  call("submission_review", {"document": doc}))
                        edit_doc({"action": "confirm_selection", "selection_hash": review["selection_hash"]})
                        session.save(doc)
                    # Prove confirmation/frames survive save and a new MCP process.
                    with Session(project, writes=True, command=str(binary)) as session:
                        doc = session.open(document)["document"]
                        reopened = session.review_submission(doc)
                        if route == "python":
                            session.export_submission(doc, name)
                            summary = session.validate_submission(name, timeout=180)
                        else:
                            call("submission_export", {"document": doc, "output": name})
                            request = call("submission_validation_request", {"package": name})
                            accepted = call("basecraft.table2asn.validate.v1", request)
                            job = accepted["run_id"]
                            deadline = time.monotonic() + 180
                            while call("basecraft.host.jobs_get.v1", {"job": job})["state"] not in (
                                    "completed", "failed", "cancelled", "interrupted", "partial"):
                                assert time.monotonic() < deadline, "validator timed out"
                                time.sleep(0.05)
                            outcome = call("basecraft.host.jobs_result.v1", {"job": job})
                            summary = call("submission_record_validation", {"package": name, "outcome": outcome})
                assert review["selection_hash"] == reopened["selection_hash"], name
                assert reopened["selection_confirmed"], name
                for row in reopened["records"]:
                    if row["record"] in frames:
                        assert row["frame"]["chosen"] == frames[row["record"]], row
                package = project / name
                (package / "DEMO-ONLY.txt").write_text(NOTICE)
                write_json(project / f"{name}-review.json", reopened)
                write_json(project / f"{name}-validation.json", summary)
                assert (package / "genbank/submission.sqn").stat().st_size > 0
                assert (package / "genbank/submission.gbf").stat().st_size > 0
                if negative:
                    assert summary["status"] == "errors", summary
                    assert any(issue["code"] == negative[0] and issue.get("record") == negative[1]
                               for issue in summary["issues"]), summary
                else:
                    assert summary["status"] == "passed" and summary["errors"] == 0, summary
                receipt["cases"].append({"route": route, "marker": name, "summary": summary})
                write_json(root / "receipt.json", receipt)
        for route in ("python", "mcp"):
            for name in CASES:
                for file in ("submission.fsa", "submission.src", "submission.tbl", "submission.sbt"):
                    assert (root / "cli" / name / "genbank" / file).read_bytes() == (
                        root / route / name / "genbank" / file).read_bytes(), (route, name, file)
        assert digest(binary) == receipt["binary_sha256"]
        assert digest(tool) == receipt["table2asn_sha256"]
        receipt["status"] = "passed"
    finally:
        write_json(root / "receipt.json", receipt)
        inventory = {str(path.relative_to(root)): {"sha256": digest(path), "bytes": path.stat().st_size}
                     for path in sorted(root.rglob("*")) if path.is_file() and path.name != "inventory.json"}
        write_json(root / "inventory.json", inventory)


if __name__ == "__main__":
    main()
