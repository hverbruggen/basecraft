"""Validate a GenBank submission with NCBI table2asn.

The host passes the submission's FASTA (.fsa), source table (.src),
five-column feature table (.tbl) and template (.sbt). table2asn runs
offline, without taxonomy (-T) or publication (-P) lookups, with
validation and a flatfile preview (-V vb):

    table2asn -t submission.sbt -i submission.fsa -src-file submission.src
              -f submission.tbl -a s -V vb -outdir out -o out/submission.sqn

Its .sqn, .val, .stats and .gbf (and .dr with discrepancy_report) are
published with a run log. Basecraft reads the .val report; creating a .sqn
is not a pass. A missing table2asn is reported as unavailable.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from basecraft.plugin import Plugin

plugin = Plugin("basecraft.table2asn", "0.1.0")
BASE = "submission"
INPUTS = {"sequences": "fsa", "sources": "src", "features": "tbl", "template": "sbt"}
REPORTS = ("sqn", "val", "stats", "gbf", "dr")


def executable():
    """Selected frozen runtime first; overrides are for unprovisioned development."""
    if "BASECRAFT_RUNTIME_EXECUTABLES" in os.environ:
        try:
            selected = json.loads(os.environ["BASECRAFT_RUNTIME_EXECUTABLES"])
            path = selected.get("table2asn") if isinstance(selected, dict) else None
            return path if isinstance(path, str) and Path(path).is_absolute() and Path(path).is_file() else None
        except (ValueError, OSError):
            return None
    if path := os.environ.get("BASECRAFT_TABLE2ASN"):
        return path if Path(path).is_file() else None
    beside = Path(sys.prefix) / "bin" / "table2asn"
    if beside.is_file():
        return str(beside)
    return shutil.which("table2asn")


def version(exe):
    """The version table2asn reports, such as `1.28.1094`."""
    out = subprocess.run([exe, "-version"], capture_output=True, text=True, check=False, timeout=60)
    match = re.search(r"table2asn:\s*(\S+)", out.stdout + out.stderr)
    return match.group(1) if match else "unknown"


def command(exe, discrepancy):
    argv = [exe, "-t", f"{BASE}.sbt", "-i", f"{BASE}.fsa", "-src-file", f"{BASE}.src",
            "-f", f"{BASE}.tbl", "-a", "s", "-V", "vb", "-outdir", "out", "-o", f"out/{BASE}.sqn"]
    if discrepancy:
        argv.append("-Z")
    return argv


def run(argv, cwd, cancel_file):
    """Run table2asn, stopping it if the host asks to cancel."""
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        process = subprocess.Popen(argv, cwd=cwd, stdout=out, stderr=err)
        while process.poll() is None:
            if cancel_file and Path(cancel_file).exists():
                process.terminate()
                process.wait(timeout=10)
                raise RuntimeError("cancelled")
            time.sleep(0.1)
        out.seek(0)
        err.seek(0)
        return (process.returncode, out.read().decode("utf-8", "replace"),
                err.read().decode("utf-8", "replace"))


def validate_files(paths, output, discrepancy=False, cancel_file=None, exe=None):
    """Validate the four files in `paths` (keyed like INPUTS) into `output`."""
    exe = exe or executable()
    if not exe:
        return {"status": "unavailable", "version": "unknown", "exit_code": None, "argv": [], "files": [],
                "message": "table2asn is not installed: install bioconda's table2asn or set BASECRAFT_TABLE2ASN "
                           "to NCBI's binary. No validation ran; this says nothing about the sequences."}, []
    try:
        tool_version = version(exe)
    except (OSError, subprocess.SubprocessError) as error:
        return {"status": "unavailable", "version": "unknown", "exit_code": None, "argv": [], "files": [],
                "message": f"table2asn could not start: {error}"}, []
    with tempfile.TemporaryDirectory(prefix="basecraft-table2asn") as work:
        for role, ext in INPUTS.items():
            shutil.copyfile(paths[role], Path(work) / f"{BASE}.{ext}")
        (Path(work) / "out").mkdir()
        argv = command(exe, discrepancy)
        code, stdout, stderr = run(argv, work, cancel_file)
        output = Path(output)
        output.mkdir(parents=True, exist_ok=True)
        files = []
        for ext in REPORTS:
            produced = Path(work) / "out" / f"{BASE}.{ext}"
            if produced.is_file():
                shutil.copyfile(produced, output / produced.name)
                files.append(produced.name)
    shown = [Path(argv[0]).name] + argv[1:]
    log = (f"table2asn {tool_version}\nargv: {' '.join(shown)}\nexit code: {code}\n\n"
           f"stdout:\n{stdout}\nstderr:\n{stderr}")
    (output / "table2asn.log").write_text(log)
    files.append("table2asn.log")
    message = f"table2asn {tool_version} exited with {code}"
    if code != 0:
        tail = [line for line in stderr.splitlines() if line.strip()][-3:]
        message += ": " + " ".join(tail)
    return {"status": "completed", "version": tool_version, "exit_code": code, "argv": shown,
            "files": files, "message": message[:4096]}, files


@plugin.operation("validate")
def validate(parameters, context):
    paths = {role: context.inputs[role][0] for role in INPUTS}
    data, files = validate_files(paths, context.outputs["reports"],
                                 bool(parameters.get("discrepancy_report", False)),
                                 context.cancel_file)
    return {"ops": [], "data": data, "artifacts": [{"role": "reports", "path": name} for name in files]}


if __name__ == "__main__":
    plugin.run()
