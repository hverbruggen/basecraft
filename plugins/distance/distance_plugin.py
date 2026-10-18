"""Public file-operation adapter for the shared Rust distance engine."""
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import bootstrap_collection_path as collection_path
from basecraft.tools import write_atomic

plugin = Plugin("basecraft.distance", "0.1.0")


def executable(params):
    path = params.get("executable") or os.environ.get("BASECRAFT_DISTANCE")
    if path:
        return str(path)
    found = shutil.which("basecraft-distance")
    if found:
        return found
    raise FileNotFoundError("Install basecraft-distance or set BASECRAFT_DISTANCE")


def retained_output(metadata, expected):
    """Never register an engine-returned file outside the allocated collection."""
    report = metadata.get("bootstrap")
    collection = report.get("replicate_trees") if isinstance(report, dict) else None
    if not isinstance(collection, dict):
        raise RuntimeError("Distance engine did not report the requested bootstrap collection")  # noqa: TRY004 - invalid engine response
    counts = [report.get(key) for key in ("requested", "attempted", "completed", "failed_replicates")]
    if (any(type(value) is not int or value < 0 for value in counts)
            or not counts[0] >= counts[1] == counts[2] + counts[3]
            or type(report.get("support_withheld")) is not bool
            or type(report.get("cancelled")) is not bool):
        raise RuntimeError("Distance engine returned invalid bootstrap artifact counts or flags")
    actual = Path(collection.get("path", ""))
    path = expected.with_suffix(".partial.nex") if report["support_withheld"] else expected
    status = ("cancelled" if report["cancelled"] else "original_failed" if report["attempted"] == 0
              else "partial" if report["support_withheld"] else "complete")
    if (actual != path or actual.is_symlink() or not actual.is_file() or actual.resolve() != actual
            or collection.get("format") != "nexus" or collection.get("status") != status
            or type(collection.get("trees")) is not int or collection["trees"] != report["completed"]
            or (not report["support_withheld"] and (report["completed"] != report["requested"] or report["cancelled"]))):
        raise RuntimeError("Distance engine returned an invalid bootstrap artifact path or status")
    return str(actual)


def recovery_error(diagnostic, metadata, report, retained, save_report=True):
    """Preserve only already-validated publication evidence from this invocation."""
    failed = dict(metadata, output_status="failed", error=diagnostic)
    if save_report:
        try:
            write_atomic(report, json.dumps(failed, indent=2) + "\n")
        except OSError as error:
            diagnostic += f"; failed run report could not be saved: {error}"
    if retained:
        evidence = failed["bootstrap"]["replicate_trees"]
        diagnostic += f"; retained bootstrap artifacts: {json.dumps(evidence)}; report: {report}"
    return RuntimeError(diagnostic)


def invoke(kind, params):
    target = Path(params["output"]).resolve()
    source = Path(params["input"]).resolve()
    if target == source:
        raise ValueError("Output must differ from input")
    model = params.get("model", "k2p")
    method = params.get("method", "sorted" if params.get("bootstrap", 0) else "njstar")
    if model not in ("p", "jc69", "k2p"):
        raise ValueError("Model must be p, jc69, or k2p")
    if method not in ("sorted", "fourpoint", "njstar", "mvrstar", "upgma"):
        raise ValueError("Unknown tree method")
    bootstrap = params.get("bootstrap", 0) if kind == "tree" else 0
    if not isinstance(bootstrap, int) or isinstance(bootstrap, bool) or bootstrap < 0:
        raise ValueError("Bootstrap must be a nonnegative integer")
    if bootstrap and method not in ("sorted", "upgma"):
        raise ValueError("Bootstrap supports only sorted NJ and UPGMA")
    collection = collection_path(params) if bootstrap else None
    cache = Path(params.get("cache", str(target) + ".distances.json")).resolve()
    report = Path(str(target) + ".run.json")
    if len({target, source, cache, report}) != 4:
        raise ValueError("Input, output, cache, and report paths must differ")
    if collection:
        reserved = {target, source, cache, report}
        reserved.update(Path(params[key]).resolve() for key in ("original_source", "progress", "cancel_file") if key in params)
        if {collection, collection.with_suffix(".partial.nex")} & reserved:
            raise ValueError("Bootstrap collection paths must differ from all inputs and other outputs")
    with tempfile.TemporaryDirectory(prefix="basecraft-distance-") as work:
        staged = Path(work) / "result"
        args = [executable(params), "--cache", str(cache), "--model", model,
                "--method", method, "--min-overlap", str(params.get("min_overlap", 1))]
        if kind == "groups":
            # input is the alignment; assignments are separate and the cache is reusable.
            assignments = Path(work) / "groups.json"
            if "groups_data" in params:
                assignments.write_text(json.dumps(params["groups_data"]))
            else:
                shutil.copyfile(params["groups"], assignments)
            args += ["--input", str(source), "--groups", str(assignments),
                     "--summary", str(staged)]
        else:
            args += ["--input", str(source), "--" + kind, str(staged)]
        if bootstrap:
            args += ["--bootstrap", str(bootstrap), "--seed", str(params.get("seed", 1)),
                     "--bootstrap-trees", str(collection)]
            for name in ("progress", "cancel_file"):
                if name in params:
                    args += ["--" + name.replace("_", "-"), str(params[name])]
        for name in ("threads", "tree_threads"):
            if name in params:
                args += ["--" + name.replace("_", "-"), str(params[name])]
        run = subprocess.run(args, capture_output=True, text=True, check=False)
        if run.returncode:
            # Preserve structured diagnostics even when the original matrix has no tree.
            # The host currently logs completed tool runs only.
            retained = []
            failed = None
            if run.stdout.strip():
                try:
                    failed = json.loads(run.stdout)
                    if collection:
                        retained = [retained_output(failed, collection)]
                except ValueError:
                    failed = None
            diagnostic = run.stderr.strip() or "Distance calculation failed"
            if failed is not None:
                raise recovery_error(diagnostic, failed, report, retained)
            raise RuntimeError(diagnostic)
        metadata = json.loads(run.stdout)
        retained = [retained_output(metadata, collection)] if collection else []
        # Publish on the destination filesystem, including for cross-device temp dirs.
        partial = None
        try:
            with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary:
                partial = Path(temporary.name)
            shutil.copyfile(staged, partial)
            os.replace(partial, target)
        except OSError as error:
            diagnostic = f"Primary output publication failed: {error}"
            if partial is not None:
                try:
                    partial.unlink(missing_ok=True)
                except OSError as cleanup_error:
                    diagnostic += f"; primary scratch cleanup failed at {partial}: {cleanup_error}"
            raise recovery_error(diagnostic, metadata, report, retained) from error
    try:
        write_atomic(report, json.dumps(metadata, indent=2) + "\n")
    except OSError as error:
        raise recovery_error(f"Run report publication failed: {error}", metadata, report, retained, save_report=False) from error
    extra_trees = []
    count = len(metadata["tree"]["trees"]) if metadata["tree"] else 0
    if count > 1:
        for i, newick in enumerate(metadata["tree"]["trees"], 1):
            path = target.with_name(f"{target.stem}.component-{i}.nwk")
            path.write_text(newick)
            extra_trees.append(str(path))
    message = f"Built {count} distance tree(s)" if kind == "tree" else "Calculated distances" if kind == "matrix" else "Summarized groups"
    bootstrap_report = metadata.get("bootstrap")
    if bootstrap_report:
        if bootstrap_report["support_withheld"]:
            message += "; bootstrap support withheld: " + "; ".join(bootstrap_report["failures"])
        else:
            message += f"; {bootstrap_report['completed']} bootstrap replicates (seed {bootstrap_report['seed']})"
        message += f"; bootstrap trees: {retained[0]}"
    return {"ops": [], "message": message, "outputs": [str(target), str(cache), str(report), *extra_trees, *retained],
            "tool": {"name": "basecraft-distance", "version": metadata["version"]}}


@plugin.operation("tree")
def tree(snapshot, params):
    return invoke("tree", params)


@plugin.operation("matrix")
def matrix(snapshot, params):
    return invoke("matrix", params)


@plugin.operation("groups")
def groups(snapshot, params):
    return invoke("groups", params)


if __name__ == "__main__":
    plugin.run()
