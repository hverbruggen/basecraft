"""Schema-2 transport for Basecraft's native Sanderson PL calculation."""

import hashlib
import json
import math
import os
import subprocess
import tempfile
import time
from pathlib import Path

from basecraft.plugin import Plugin
from tree import labeled_newick, parse

plugin = Plugin("basecraft.sanderson", "0.1.0")


def _indexed(root):
    nodes = []
    labels = {}
    def visit(node, parent):
        index = len(nodes)
        nodes.append(None)
        if node.children and node.label:
            labels.setdefault(node.label, []).append(index)
        children = [visit(child, index) for child in node.children]
        nodes[index] = {"parent": parent, "children": children, "label": node.label,
                        "length": node.length}
        return index
    visit(root, None)
    keys = {node.key: i for i, node in enumerate(root.walk())}
    return nodes, keys, labels


def _parameters(parameters, keys, labels):
    settings = dict(parameters)
    for name, value in (("penalty", "log"), ("starts", 3), ("seed", 1),
                        ("max_iterations", 2000), ("max_seconds", 3600)):
        settings.setdefault(name, value)
    if ("lambda" in settings) == ("cross_validation" in settings):
        raise ValueError("Supply exactly one of lambda and cross_validation")
    constraints = []
    for item in settings.get("constraints", []):
        name = item["node"]
        matches = set(labels.get(name, []))
        if name in keys:
            matches.add(keys[name])
        if len(matches) != 1:
            raise ValueError("Constraint node must be a unique internal label or preorder ID")
        constraints.append({"node": next(iter(matches)), "min_age": item["min_age"],
                            "max_age": item["max_age"]})
    settings["constraints"] = constraints
    return settings


def _binary(package=None):
    package = Path(package) if package is not None else Path(__file__).resolve().parent
    path = package / "bin" / ("basecraft-sanderson-native.exe" if os.name == "nt" else "basecraft-sanderson-native")
    if not path.is_file():
        raise RuntimeError("Native Sanderson PL engine is not packaged")
    try:
        identity = "validated-engine-windows.json" if os.name == "nt" else "validated-engine.json"
        expected = json.loads((package / "native" / identity).read_text())
        staged = json.loads((package / "bin" / "engine.json").read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError("Native Sanderson PL engine identity is missing or invalid") from error
    if staged != expected:
        raise RuntimeError("Native Sanderson PL engine identity differs from validation")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != expected["binary_sha256"]:
        raise RuntimeError("Native Sanderson PL binary differs from validation")
    return path, digest


def _run(binary, request, cancel, seconds):
    with tempfile.TemporaryDirectory(prefix="basecraft-sanderson-") as directory:
        work = Path(directory)
        with (work / "request.json").open("w") as request_file:
            json.dump(request, request_file, ensure_ascii=False, allow_nan=False)
        with (work / "request.json").open("r") as stdin, (work / "stdout").open("w+") as stdout, (work / "stderr").open("w+") as stderr:
            if cancel and Path(cancel).exists():
                raise RuntimeError("Sanderson PL cancelled before calculation")
            process = subprocess.Popen([str(binary)], stdin=stdin, stdout=stdout, stderr=stderr, cwd=work)
            deadline = time.monotonic() + seconds + 10
            while process.poll() is None:
                if cancel and Path(cancel).exists():
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise RuntimeError("Sanderson PL cancelled")
                if time.monotonic() >= deadline:
                    process.kill()
                    process.wait()
                    raise RuntimeError("Sanderson PL exceeded its time budget")
                time.sleep(0.1)
            stdout.seek(0)
            stderr.seek(0)
            output = stdout.read(8_000_001)
            error = stderr.read(1_000_001)
        if len(output) > 8_000_000 or len(error) > 1_000_000:
            raise RuntimeError("Native Sanderson PL output exceeded size limit")
        if cancel and Path(cancel).exists():
            raise RuntimeError("Sanderson PL cancelled")
        if process.returncode:
            raise RuntimeError("Native Sanderson PL failed: " + (error or output)[-1000:])
        return json.loads(output, parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"invalid numeric value {value}")))


def _validate_result(root, nodes, result, settings):
    ages = result["ages"]
    rates = result["rates_per_site"]
    if len(ages) != len(nodes) or len(rates) != len(nodes):
        raise ValueError("Native PL result has the wrong number of nodes")
    if not all(isinstance(age, (int, float)) and math.isfinite(age) for age in ages):
        raise ValueError("Native PL result has a nonfinite age")
    if not math.isclose(ages[0], settings["root_age"], rel_tol=1e-10):
        raise ValueError("Native PL result changed the root age")
    durations = {}
    ordered = list(root.walk())
    for index, node in enumerate(nodes):
        age = ages[index]
        if not node["children"] and abs(age) > 1e-8:
            raise ValueError("Native PL result has a nonzero extant-tip age")
        if index:
            duration = ages[node["parent"]] - age
            rate = rates[index]
            if duration <= 0 or not math.isfinite(duration) or not isinstance(rate, (int, float)) or not math.isfinite(rate) or rate <= 0:
                raise ValueError("Native PL result has invalid duration or fitted rate")
            durations[ordered[index].key] = duration
    for constraint in settings["constraints"]:
        age = ages[constraint["node"]]
        if age < constraint["min_age"] - 1e-8 or age > constraint["max_age"] + 1e-8:
            raise ValueError("Native PL result violates a hard age bound")
    if not all(isinstance(result[key], (int, float)) and math.isfinite(result[key])
               for key in ("lambda", "objective")) or result["lambda"] <= 0:
        raise ValueError("Native PL result has invalid objective or lambda")
    return durations


@plugin.context_operation("date_tree")
def date_tree(_snapshot, parameters, context):
    source = Path(context["inputs"]["tree"][0])
    raw = source.read_bytes()
    root = parse(raw.decode("utf-8"))
    nodes, keys, labels = _indexed(root)
    settings = _parameters(parameters, keys, labels)
    binary, digest = _binary()
    cancel = context.get("cancel_file")
    result = _run(binary, {"nodes": nodes, "parameters": settings, "cancel_file": cancel},
                  cancel, settings.get("max_seconds", 3600))
    durations = _validate_result(root, nodes, result, settings)
    chronogram = labeled_newick(root, durations) + ";\n"
    report = {"schema": 1, "method": "Sanderson penalized likelihood",
              "observation_convention": result["observation_convention"],
              "source_sha256": hashlib.sha256(raw).hexdigest(),
              "source_newick": raw.decode("utf-8"),
              "received_parameters": parameters,
              "effective_settings": settings,
              "native_engine": {"name": "basecraft-sanderson-native", "version": "0.1.0",
                                "binary_sha256": digest, "optimizer": result["optimizer"]},
              "result": result,
              "nodes": [{"id": node.key, "label": node.label, "original_length_text": node.length_text,
                         "age": result["ages"][index], "fitted_rate_per_site": result["rates_per_site"][index]}
                        for index, node in enumerate(root.walk())]}
    report_text = json.dumps(report, ensure_ascii=False, allow_nan=False, indent=2) + "\n"
    Path(context["outputs"]["chronogram"]).write_text(chronogram)
    Path(context["outputs"]["report"]).write_text(report_text)
    return {"data": {"lambda": result["lambda"], "root_age": settings["root_age"],
                     "time_unit": settings["time_unit"], "node_count": len(nodes),
                     "engine_sha256": digest},
            "artifacts": [{"role": "chronogram", "path": "."}, {"role": "report", "path": "."}]}


if __name__ == "__main__":
    plugin.run()
