"""Public file-operation adapter for the shared Rust distance engine."""
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin

plugin = Plugin("basecraft.distance", "0.1.0")


def executable(params):
    path = params.get("executable") or os.environ.get("BASECRAFT_DISTANCE")
    if path:
        return str(path)
    found = shutil.which("basecraft-distance")
    if found:
        return found
    raise FileNotFoundError("Install basecraft-distance or set BASECRAFT_DISTANCE")


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
    cache = Path(params.get("cache", str(target) + ".distances.json")).resolve()
    report = Path(str(target) + ".run.json")
    if len({target, source, cache, report}) != 4:
        raise ValueError("Input, output, cache, and report paths must differ")
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
            args += ["--bootstrap", str(bootstrap), "--seed", str(params.get("seed", 1))]
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
            if run.stdout.strip():
                try:
                    report.write_text(json.dumps(json.loads(run.stdout), indent=2) + "\n")
                except (ValueError, OSError):
                    pass
            raise RuntimeError(run.stderr.strip() or "Distance calculation failed")
        metadata = json.loads(run.stdout)
        # Publish on the destination filesystem, including for cross-device temp dirs.
        partial = target.with_name(target.name + ".part")
        shutil.copyfile(staged, partial)
        os.replace(partial, target)
    report.write_text(json.dumps(metadata, indent=2) + "\n")
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
    return {"ops": [], "message": message, "outputs": [str(target), str(cache), str(report), *extra_trees],
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
