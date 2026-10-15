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
METHODS = ("workspace_tree", "workspace_alignment", "haplotypes", "validate_time_tree", "relative_time", "manual_cut",
           "gmyc_single", "gmyc_multiple", "gmyc_exhaustive", "inspect_tree", "ptp", "mptp", "asap", "asap_import", "group_statistics")


def executable(params):
    found = params.get("executable") or os.environ.get("BASECRAFT_DELIMITATION") or shutil.which("basecraft-delimitation")
    if not found:
        raise FileNotFoundError("Install basecraft-delimitation or set BASECRAFT_DELIMITATION")
    return str(found)


def invoke(method, params):
    source = Path(params["input"]).resolve()
    target = Path(params["output"]).resolve()
    report = Path(str(target) + ".run.json")
    job_id = params.get("job_id") or str(uuid.uuid4())
    if not isinstance(job_id, str) or len(job_id) > 100 or not all(c.isascii() and (c.isalnum() or c in "-_") for c in job_id):
        raise ValueError("job_id must contain only 1-100 ASCII letters, digits, '-' or '_'")
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
    request = {"version": 1, "job_id": job_id, "generation": params.get("generation", 0),
                   "method": method, "input": str(snapshot), "input_hash": input_hash, "output": str(target.parent / (".delimitation-job-" + job_id) / "result.json"),
                   "rooted": params.get("rooted", False), "tree_index": params.get("tree_index", 0),
                   "seed": params.get("seed", 1), "workers": workers,
                   "parameters": {k: v for k, v in params.items() if k in ("threshold", "candidate_budget", "reference_version", "reference_age_clamp", "groups", "log_likelihood", "fitted_log_likelihood", "null_log_likelihood", "lrt_passed", "mptp_version", "command", "min_branch", "units", "diagnostics", "asap_model", "replicates", "upstream_version", "haplotypes", "haplotype_policy", "mapping_hash", "upstream", "tree_inference", "sequence_sampling", "alignment_input_hash", "alignment_snapshot", "deduplicate", "original_count", "retained_count")},
                   "progress": params.get("progress"), "cancel_file": params.get("cancel_file")}
    mapping_snapshot = None
    if params.get("mapping"):
        mapping_source=Path(params["mapping"]).resolve()
        mapping_snapshot=Path(str(target)+".mapping."+job_id+".json")
        if mapping_source in set(paths) or mapping_snapshot == mapping_source:
            raise ValueError("Mapping input must differ from output artifacts")
        shutil.copyfile(mapping_source,mapping_snapshot)
        mapping=json.loads(mapping_snapshot.read_text())
        if mapping.get("policy") != "aligned-dna-case-u-to-t-literal-gap-ambiguity-v1":
            raise ValueError("Unsupported exact haplotype identity policy")
        mapping_hash = subprocess.check_output([engine,"--hash",str(mapping_snapshot)],text=True).strip()
        if params.get("mapping_hash") and mapping_hash != params["mapping_hash"]:
            raise ValueError("Retained haplotype mapping changed; repeat preparation")
        request["parameters"].update(haplotypes=mapping["members"],haplotype_policy=mapping["policy"],mapping_hash=mapping_hash)
    request["parameters"]["original_source"] = params.get("original_source", str(source))
    staged_output = Path(request["output"])
    staged_output.parent.mkdir()
    with tempfile.TemporaryDirectory(prefix="basecraft-delimitation-") as work:
        request_path = Path(work) / "request.json"
        request_path.write_text(json.dumps(request))
        process = subprocess.run([engine, "--request", str(request_path)], capture_output=True, text=True, check=False)
    if staged_output.exists():
        metadata = json.loads(staged_output.read_text())
    else:
        metadata = {"version": 1, "job": {"id": request["job_id"], "generation": request["generation"],
                    "method": method, "status": "failed", "inputs": {str(snapshot): input_hash},
                    "outputs": {}, "elapsed_seconds": 0.0,
                    "diagnostics": [process.stderr[-16000:] or "No engine result"]}, "workspace": None, "metrics": {}}
    if metadata["job"]["id"] != job_id:
        raise ValueError("Engine returned a different job identity")
    metadata["job"]["method"] = params.get("public_method", method)
    # Only this invocation's staged artifacts can become a completed result.
    # An old destination is never mistaken for a newly failed engine run.
    if process.returncode == 0 and metadata["job"]["status"] == "completed":
        published = {}
        for artifact, digest in metadata["job"]["outputs"].items():
            artifact_path = Path(artifact)
            if artifact_path.parent != staged_output.parent:
                raise ValueError("Engine artifact escaped its job directory")
            suffix = artifact_path.name.removeprefix(staged_output.name)
            destination = Path(str(target) + suffix)
            os.replace(artifact_path, destination)
            published[str(destination)] = digest
        metadata["job"]["outputs"] = published
        with staged_output.open("w") as stream:
            json.dump(metadata, stream, indent=2)
            stream.write("\n")
        os.replace(staged_output, target)
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
    if mapping_snapshot:
        outputs.append(str(mapping_snapshot))
    return {"ops": [], "message": "Completed " + method.replace("_", " "), "outputs": outputs,
            "tool": {"name": "basecraft-delimitation", "version": "0.1.0"}}


def failure_report(method, params, error, artifacts=()):
    target = Path(params["output"]).resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    engine = None
    hashing_errors = []
    try:
        engine = executable(params)
    except FileNotFoundError as engine_error:
        hashing_errors.append(str(engine_error))
    hashes = {}
    for artifact in artifacts:
        if Path(artifact).is_file() and engine:
            try:
                hashes[str(artifact)] = subprocess.check_output([engine, "--hash", str(artifact)], text=True).strip()
            except (OSError, subprocess.CalledProcessError) as hash_error:
                hashing_errors.append(str(hash_error))
    job = {"id": params.get("job_id", str(uuid.uuid4())), "generation": params.get("generation", 0),
               "method": method, "status": "cancelled" if params.get("cancel_file") and Path(params["cancel_file"]).exists() else "failed",
               "inputs": {}, "outputs": hashes, "diagnostics": [str(error), *hashing_errors], "elapsed_seconds": 0.0}
    with tempfile.NamedTemporaryFile(mode="w", dir=target.parent, prefix=".delimitation-failed-", delete=False) as stream:
        json.dump({"version": 1, "job": job, "workspace": None}, stream, indent=2)
        stream.write("\n")
        temporary = stream.name
    os.replace(temporary, str(target) + ".run.json")


for _method in METHODS:
    def register(method):
        @plugin.operation(method)
        def operation(snapshot, params):
            params = dict(params, job_id=params.get("job_id") or str(uuid.uuid4()))
            try:
                if method in ("gmyc_single", "gmyc_multiple", "gmyc_exhaustive", "ptp", "mptp") and params.get("deduplicate") and not params.get("alignment_snapshot"):
                    raise ValueError("Tree-only deduplication requires verified alignment evidence and a representative tree; use alignment GMYC/ASAP or prepare a representative tree and mapping first. No tips were removed.")
                if method == "asap" and params.get("deduplicate") and not params.get("mapping"):
                    raise ValueError("Deduplicated ASAP requires exact haplotype preparation and a retained mapping")
                if method == "group_statistics":
                    import sys
                    sys.path.insert(0,str(Path(__file__).resolve().parent.parent / "distance"))
                    from distance_plugin import invoke as invoke_distance
                    if params.get("cache") and Path(params["cache"]).is_file():
                        cache=json.loads(Path(params["cache"]).read_text())
                        params=dict(params,model=cache["model"],min_overlap=cache["min_overlap"])
                    return invoke_distance("groups", params)
                if method == "asap":
                    from asap_adapter import invoke as invoke_asap
                    return invoke_asap(params, executable(params), invoke)
                if method in ("ptp", "mptp"):
                    from mptp_adapter import invoke as invoke_mptp
                    return invoke_mptp(method, params, executable(params), invoke)
                return invoke(method, params)
            except Exception as error:
                report = Path(str(params["output"]) + ".run.json")
                try:
                    current = json.loads(report.read_text())
                except (OSError, ValueError):
                    current = {}
                if current.get("job", {}).get("id") != params["job_id"]:
                    artifacts = Path(str(params["output"]) + (".asap-" if method == "asap" else ".mptp-") + params["job_id"])
                    failure_report(method, params, error, artifacts.rglob("*") if artifacts.is_dir() else ())
                raise
    register(_method)

if __name__ == "__main__":
    plugin.run()
