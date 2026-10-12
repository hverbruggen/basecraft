"""Public versioned adapter for immutable native delimitation inputs."""
import json
import os
import shutil
import subprocess
import tempfile
import uuid
from pathlib import Path
from basecraft.plugin import Plugin

plugin = Plugin("basecraft.delimitation", "0.1.0")
METHODS = ("haplotypes", "validate_time_tree", "relative_time", "manual_cut",
           "gmyc_single", "gmyc_multiple", "gmyc_exhaustive")


def executable(params):
    found = params.get("executable") or os.environ.get("BASECRAFT_DELIMITATION") or shutil.which("basecraft-delimitation")
    if not found:
        raise FileNotFoundError("Install basecraft-delimitation or set BASECRAFT_DELIMITATION")
    return str(found)


def invoke(method, params):
    source = Path(params["input"]).resolve()
    target = Path(params["output"]).resolve()
    report = Path(str(target) + ".run.json")
    job_id = str(uuid.uuid4())
    snapshot = Path(str(target) + ".input." + job_id + source.suffix)
    spart = Path(str(target) + ".spart")
    sidecar = Path(str(spart) + ".basecraft.json")
    paths = [source, target, report, snapshot, spart, sidecar]
    for name in ("progress", "cancel_file"):
        if name in params:
            paths.append(Path(params[name]).resolve())
    if len(set(paths)) != len(paths):
        raise ValueError("Input, output artifacts, progress and cancellation paths must differ")
    if method not in METHODS:
        raise ValueError("Unknown delimitation method")
    workers = params.get("workers", 1)
    if not isinstance(workers, int) or isinstance(workers, bool) or not 1 <= workers <= 256:
        raise ValueError("Workers must be between 1 and 256")
    engine = executable(params)
    target.parent.mkdir(parents=True, exist_ok=True)
    # Destination-local snapshot survives worker completion and belongs in the manifest.
    with tempfile.NamedTemporaryFile(dir=target.parent, prefix=".delimitation-input-", delete=False) as staged:
        staged_path = Path(staged.name)
        try:
            with source.open("rb") as original:
                shutil.copyfileobj(original, staged)
            staged.flush()
            os.fsync(staged.fileno())
            os.replace(staged_path, snapshot)
        finally:
            staged_path.unlink(missing_ok=True)
    input_hash = subprocess.check_output([engine, "--hash", str(snapshot)], text=True).strip()
    request = dict(version=1, job_id=job_id, generation=params.get("generation", 0),
                   method=method, input=str(snapshot), input_hash=input_hash, output=str(target),
                   rooted=params.get("rooted", False), tree_index=params.get("tree_index", 0),
                   seed=params.get("seed", 1), workers=workers,
                   parameters={k: v for k, v in params.items() if k in ("threshold", "candidate_budget")},
                   progress=params.get("progress"), cancel_file=params.get("cancel_file"))
    request["parameters"]["original_source"] = str(source)
    with tempfile.TemporaryDirectory(prefix="basecraft-delimitation-") as work:
        request_path = Path(work) / "request.json"
        request_path.write_text(json.dumps(request))
        process = subprocess.run([engine, "--request", str(request_path)], capture_output=True, text=True, check=False)
    if target.exists():
        metadata = json.loads(target.read_text())
    else:
        metadata = {"version": 1, "job": {"id": request["job_id"], "generation": request["generation"],
                    "method": method, "status": "failed", "inputs": {str(snapshot): input_hash},
                    "outputs": {}, "elapsed_seconds": 0.0,
                    "diagnostics": [process.stderr[-16000:] or "No engine result"]}, "workspace": None, "metrics": {}}
    metadata["request"] = request
    with tempfile.NamedTemporaryFile(mode="w", dir=target.parent, prefix=".delimitation-report-", delete=False) as staged:
        report_path = Path(staged.name)
        json.dump(metadata, staged, indent=2)
        staged.write("\n")
    os.replace(report_path, report)
    if process.returncode or metadata["job"]["status"] != "completed":
        raise RuntimeError("; ".join(metadata["job"]["diagnostics"]) or "Delimitation failed")
    outputs = [str(target), str(report), str(snapshot)]
    outputs += list(metadata["job"]["outputs"])
    return {"ops": [], "message": "Completed " + method.replace("_", " "), "outputs": outputs,
            "tool": {"name": "basecraft-delimitation", "version": "0.1.0"}}


for _method in METHODS:
    def register(method):
        @plugin.operation(method)
        def operation(snapshot, params):
            return invoke(method, params)
    register(_method)

if __name__ == "__main__":
    plugin.run()
