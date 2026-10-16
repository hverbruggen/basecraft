"""Public file-operation adapter for the shared Rust distance engine."""
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin
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


def collection_path(params):
    """Consume public host allocation; never create or guess a hidden namespace."""
    destination = params.get("bootstrap_destination", "metadata")
    if destination not in ("metadata", "working"):
        raise ValueError("Bootstrap destination must be metadata or working")
    if "bootstrap_trees" in params:
        if "analysis_artifacts" in params:
            raise ValueError("bootstrap_trees and analysis_artifacts are ambiguous together")
        path = Path(params["bootstrap_trees"])
        if not path.is_absolute() or path.is_symlink():
            raise ValueError("bootstrap_trees must be an absolute, non-symlink output path")
        return path.parent.resolve(strict=True) / path.name
    allocation = params.get("analysis_artifacts")
    if not isinstance(allocation, dict):
        raise TypeError("Bootstrap retention requires host analysis_artifacts allocation or an explicit absolute bootstrap_trees path")
    run_id = allocation.get("run_id")
    if not isinstance(run_id, str) or not re.fullmatch(r"[A-Za-z0-9-]+", run_id):
        raise ValueError("Invalid bootstrap artifact run_id")
    key = "metadata_directory" if destination == "metadata" else "working_directory"
    value = allocation.get(key)
    if not isinstance(value, str):
        raise TypeError(f"Missing allocated {key}")
    directory = Path(value)
    if not directory.is_absolute() or not directory.is_dir() or directory.resolve() != directory:
        raise ValueError(f"Allocated {key} must be an existing absolute directory without symlinks")
    return directory / f"bootstrap-{run_id}.nex"


def retained_output(metadata, expected):
    """Never register an engine-returned file outside the allocated collection."""
    report = metadata.get("bootstrap")
    collection = report.get("replicate_trees") if isinstance(report, dict) else None
    if not isinstance(collection, dict):
        raise RuntimeError("Distance engine did not report the requested bootstrap collection")  # noqa: TRY004 - invalid engine response
    actual = Path(collection.get("path", ""))
    path = expected.with_suffix(".partial.nex") if report["support_withheld"] else expected
    status = ("cancelled" if report["cancelled"] else "original_failed" if report["attempted"] == 0
              else "partial" if report["support_withheld"] else "complete")
    if (actual != path or actual.is_symlink() or not actual.is_file()
            or collection.get("format") != "nexus" or collection.get("status") != status
            or collection.get("trees") != report["completed"]):
        raise RuntimeError("Distance engine returned an invalid bootstrap artifact path or status")
    return str(actual)


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
            if run.stdout.strip():
                try:
                    failed = json.loads(run.stdout)
                    if collection:
                        retained = [retained_output(failed, collection)]
                    write_atomic(report, json.dumps(failed, indent=2) + "\n")
                except ValueError:
                    pass
                except OSError as error:
                    raise RuntimeError(f"Failed run report could not be saved; retained bootstrap artifacts: {retained}: {error}") from error
            diagnostic = run.stderr.strip() or "Distance calculation failed"
            if retained:
                diagnostic += f"; retained bootstrap artifacts: {retained}; report: {report}"
            raise RuntimeError(diagnostic)
        metadata = json.loads(run.stdout)
        retained = [retained_output(metadata, collection)] if collection else []
        # Publish on the destination filesystem, including for cross-device temp dirs.
        with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary:
            partial = Path(temporary.name)
        try:
            shutil.copyfile(staged, partial)
            os.replace(partial, target)
        except OSError as error:
            raise RuntimeError(f"Primary output publication failed; retained bootstrap artifacts: {retained}: {error}") from error
        finally:
            partial.unlink(missing_ok=True)
    try:
        write_atomic(report, json.dumps(metadata, indent=2) + "\n")
    except OSError as error:
        raise RuntimeError(f"Run report publication failed; retained bootstrap artifacts: {retained}: {error}") from error
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
