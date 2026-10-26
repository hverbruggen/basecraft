"""Pinned r8s 1.81 PL/TN execution and conservative result checks."""

import hashlib
import json
import math
from decimal import Decimal
from pathlib import Path
import re
import subprocess
import tempfile
import time

from tree import neutral_newick, parse, tips

SOURCE_SHA256 = "9e89d7851d74d74487d147b77177a717e6c659b485c9b67f516340a6ed595080"
CV_LINE = re.compile(r"^\s*(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+\((Good|Failed)\)\s*$")
TREE_LINE = re.compile(r"(?im)^\s*tree\s+\S+\s*=\s*(\([^;\n]+;)")
NUMBER = re.compile(r"^[+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$")
ERROR_WORDS = re.compile(r"(?i)\b(?:fatal error|optimization failed|failed to converge|nan|infinity)\b")


def finite_positive(value, name):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
        raise ValueError(f"{name} must be a positive finite number")
    return float(value)


def settings(parameters, root):
    if not isinstance(parameters, dict):
        raise ValueError("Parameters must be an object")
    units = parameters["branch_units"]
    if units not in ("substitutions_per_site", "substitution_counts"):
        raise ValueError("branch_units must be substitutions_per_site or substitution_counts")
    nsites = parameters["sites"]
    if isinstance(nsites, bool) or not isinstance(nsites, int) or not 1 <= nsites <= 1_000_000_000:
        raise ValueError("sites must be an integer from 1 to 1000000000")
    for node in root.walk():
        if node is root:
            continue
        count = Decimal(node.length_text) * (nsites if units == "substitutions_per_site" else 1)
        if count != count.to_integral_value() or count <= 0 or count > 2147483647:
            raise ValueError(f"Branch {node.key} does not scale to an exact positive integer count; r8s PL would round it")
    age = finite_positive(parameters["root_age"], "root_age")
    if not 0.001 <= age <= 100_000_000:
        raise ValueError("root_age must be within 0.001..100000000 for r8s output precision")
    if not isinstance(parameters["time_unit"], str) or not parameters["time_unit"].strip():
        raise ValueError("time_unit must be a nonempty label")
    if any(ord(ch) < 32 for ch in parameters["time_unit"]):
        raise ValueError("time_unit cannot contain control characters")
    penalty = parameters["penalty"]
    if penalty not in ("add", "log"):
        raise ValueError("penalty must be add or log")
    starts = parameters["starts"]
    seed = parameters["seed"]
    if any(isinstance(x, bool) or not isinstance(x, int) for x in (starts, seed)) or not (1 <= starts <= 25 and 1 <= seed <= 2147483647):
        raise ValueError("starts must be 1..25 and seed must be 1..2147483647")
    constraints = parameters.get("constraints", [])
    if not isinstance(constraints, list) or len(constraints) > 128:
        raise ValueError("constraints must be an array of at most 128 entries")
    nodes = {node.key: node for node in root.walk()}
    named = {}
    for node in root.walk():
        if node.children and node.label:
            named.setdefault(node.label, []).append(node.key)
    seen = set()
    resolved_constraints = []
    for item in constraints:
        if not isinstance(item, dict) or set(item) != {"node", "min_age", "max_age"}:
            raise ValueError("Each constraint needs node, min_age and max_age")
        key = item["node"]
        if key not in nodes and key in named and len(named[key]) == 1:
            key = named[key][0]
        if key not in nodes or key in seen or not nodes[key].children:
            raise ValueError("Constraints require unique internal node IDs")
        seen.add(key)
        lower = finite_positive(item["min_age"], "min_age")
        upper = finite_positive(item["max_age"], "max_age")
        if lower > upper or upper > age or (key == root.key and (lower != age or upper != age)):
            raise ValueError("Constraint bounds conflict with root age")
        resolved_constraints.append({"node": key, "min_age": lower, "max_age": upper})
    # Strict ancestry feasibility: every descendant must allow a strictly younger age.
    bounds = {item["node"]: (item["min_age"], item["max_age"]) for item in resolved_constraints}
    def feasible(node):
        low = bounds.get(node.key, (0, age))[0]
        for child in node.children:
            child_low = feasible(child)
            if child_low >= bounds.get(node.key, (0, age))[1]:
                raise ValueError("Infeasible ancestor/descendant age constraints")
            low = max(low, math.nextafter(child_low, math.inf))
        return low
    if feasible(root) > age:
        raise ValueError("Age constraints are infeasible below root age")
    cv = parameters.get("cross_validation")
    if cv is not None:
        if not isinstance(cv, dict) or set(cv) != {"log10_start", "log10_step", "count"}:
            raise ValueError("cross_validation needs log10_start, log10_step and count")
        start, step, count = cv["log10_start"], cv["log10_step"], cv["count"]
        if any(isinstance(x, bool) or not isinstance(x, (int, float)) or not math.isfinite(x) for x in (start, step)):
            raise ValueError("CV logarithmic grid must be finite")
        if isinstance(count, bool) or not isinstance(count, int) or not 2 <= count <= 30 or step <= 0:
            raise ValueError("CV count must be 2..30 and step positive")
        if start < -8 or start + step * (count - 1) > 8:
            raise ValueError("CV lambda grid exceeds 10^-8..10^8")
    lam = parameters.get("lambda")
    if cv is None and lam is None:
        raise ValueError("Supply lambda or a cross_validation grid")
    if lam is not None:
        finite_positive(lam, "lambda")
        if lam > 1e8:
            raise ValueError("lambda must be at most 1e8")
    return {"units": units, "sites": nsites, "root_age": age, "time_unit": parameters["time_unit"],
            "penalty": penalty, "starts": starts, "seed": seed, "constraints": resolved_constraints,
            "cross_validation": cv, "lambda": lam}


def script(root, options, lam=None, cv=False):
    lines = ["#NEXUS", "begin trees;", "tree input = " + neutral_newick(root) + ";", "end;", "begin rates;",
             f"blformat lengths={'persite' if options['units'] == 'substitutions_per_site' else 'total'} nsites={options['sites']} ultrametric=no round=no;"]
    for node in root.walk():
        if node.children:
            left, right = tips(node.children[0])[0], tips(node.children[1])[0]
            lines.append(f"mrca {node.key} {left} {right};")
    lines.append(f"fixage taxon={root.key} age={options['root_age']:.17g};")
    for item in options["constraints"]:
        key, low, high = item["node"], item["min_age"], item["max_age"]
        if low == high:
            lines.append(f"fixage taxon={key} age={low:.17g};")
        else:
            lines.append(f"constrain taxon={key} min_age={low:.17g} max_age={high:.17g};")
    lines.append(f"set penalty={options['penalty']} seed={options['seed']} num_time_guesses={options['starts']} checkgradient=yes;")
    if cv:
        grid = options["cross_validation"]
        lines.append(f"divtime method=pl algorithm=tn crossv=yes cvstart={grid['log10_start']:.17g} cvinc={grid['log10_step']:.17g} cvnum={grid['count']};")
    else:
        lines += [f"set smoothing={lam:.17g};", "divtime method=pl algorithm=tn;",
                  "showage;", "describe plot=chrono_description;"]
    return "\n".join(lines + ["end;", ""])


def engine_identity(package):
    binary = package / "bin" / "r8s"
    manifest = package / "bin" / "engine.json"
    if not binary.is_file() or not manifest.is_file():
        raise RuntimeError("Pinned r8s 1.81 engine is not packaged")
    identity = json.loads(manifest.read_text())
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if identity != {"name": "r8s", "version": "1.81", "source_sha256": SOURCE_SHA256, "binary_sha256": digest}:
        raise RuntimeError("Packaged r8s identity or checksum does not match")
    return binary, identity


def execute(binary, text, cancel_file, timeout_seconds=3600):
    with tempfile.TemporaryDirectory(prefix="basecraft-r8s-") as directory:
        work = Path(directory)
        query = work / "input.nex"
        query.write_text(text)
        with (work / "stdout").open("w+") as out, (work / "stderr").open("w+") as err:
            process = subprocess.Popen([str(binary), "-f", str(query)], cwd=work, stdout=out, stderr=err)
            deadline = time.monotonic() + timeout_seconds
            while process.poll() is None:
                if cancel_file and Path(cancel_file).exists():
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise RuntimeError("r8s run cancelled")
                if time.monotonic() > deadline:
                    process.kill()
                    process.wait()
                    raise RuntimeError("r8s exceeded runtime limit")
                time.sleep(0.1)
            out.seek(0)
            err.seek(0)
            stdout, stderr = out.read(4_000_001), err.read(1_000_001)
        if len(stdout) > 4_000_000 or len(stderr) > 1_000_000:
            raise RuntimeError("r8s output exceeded size limit")
        if process.returncode or ERROR_WORDS.search(stderr) or ERROR_WORDS.search(stdout):
            raise RuntimeError(f"r8s failed (exit {process.returncode}): {(stderr or stdout)[-1000:]}")
        if cancel_file and Path(cancel_file).exists():
            raise RuntimeError("r8s run cancelled")
        return stdout, stderr


def parse_cv(stdout, grid):
    marker = "Results of cross validation analysis for tree"
    if stdout.count(marker) != 1:
        raise ValueError("Missing or ambiguous r8s cross-validation summary")
    block = stdout.split(marker, 1)[1].split("Optimum:", 1)[0]
    rows = []
    for line in block.splitlines():
        match = CV_LINE.match(line)
        if match:
            log_lambda, printed_lambda, raw, chi, state = match.groups()
            numbers = [float(x) for x in (log_lambda, printed_lambda, raw, chi)]
            if not all(math.isfinite(x) for x in numbers):
                raise ValueError("Nonfinite CV score")
            rows.append({"log10_lambda": grid["log10_start"] + len(rows) * grid["log10_step"],
                         "lambda": 10 ** (grid["log10_start"] + len(rows) * grid["log10_step"]),
                         "raw_squared_error": numbers[2], "chi_square_error": numbers[3], "status": state})
    if len(rows) != grid["count"] or any(row["status"] != "Good" for row in rows) or "WARNING: Cross validation procedure had errors" in stdout:
        raise ValueError("r8s cross-validation did not complete at every grid point")
    optimum = re.search(r"(?m)^Optimum:\s+([+-]?\d+(?:\.\d+)?)\s+", stdout)
    if optimum is None:
        raise ValueError("Missing r8s selected CV optimum")
    printed_log = float(optimum.group(1))
    winners = [row for row in rows if abs(row["log10_lambda"] - printed_log) <= 0.0051]
    if len(winners) != 1:
        raise ValueError("Printed r8s CV optimum is ambiguous on this grid")
    return rows, winners[0]["lambda"]


def parse_fit(stdout, source_root, root_age, constraints):
    best = re.findall(r"Using optimization from starting point\s+(\d+)\s+as best estimate", stdout)
    status_section = stdout.split("Result codes:", 1)
    if len(best) != 1 or len(status_section) != 2:
        raise ValueError("Missing r8s repeated-start convergence diagnostics")
    status_lines = [line for line in status_section[0].splitlines() if "PASSED" in line or "FAILED(" in line]
    states = re.findall(r"PASSED|FAILED\([12]\)", status_lines[-1]) if status_lines else []
    best_index = int(best[0]) - 1
    if best_index < 0 or best_index >= len(states) or states[best_index] != "PASSED":
        raise ValueError("r8s selected a nonconverged starting point")
    matches = TREE_LINE.findall(stdout)
    if len(matches) != 1:
        raise ValueError("Expected exactly one r8s chronogram description")
    result = parse(matches[0])
    source_nodes = list(source_root.walk())
    result_nodes = list(result.walk())
    def neutral_topology(node):
        return (("internal" if node.children else node.key), tuple(neutral_topology(child) for child in node.children))
    def returned_topology(node):
        return (("internal" if node.children else node.label), tuple(returned_topology(child) for child in node.children))
    if returned_topology(result) != neutral_topology(source_root):
        raise ValueError("r8s changed the supplied rooted topology")
    duration = {node.key: node.length for node in result_nodes if node is not result}
    ages = {result.key: root_age}
    def visit(node):
        for child in node.children:
            ages[child.key] = ages[node.key] - duration[child.key]
            if ages[child.key] >= ages[node.key] or ages[child.key] < -1e-5:
                raise ValueError("Chronogram contains invalid ages or durations")
            visit(child)
    visit(result)
    tolerance = max(1e-5, 1e-6 * len(source_nodes))
    if any(abs(ages[node.key]) > tolerance for node in result_nodes if not node.children):
        raise ValueError("r8s output is not ultrametric at age zero")
    for item in constraints:
        age = ages[item["node"]]
        if age < item["min_age"] - tolerance or age > item["max_age"] + tolerance:
            raise ValueError("r8s output violates a hard age constraint")
    # showage's PL rate column is five significant figures. Do not represent it
    # as a full-precision optimum or infer it from length/duration.
    rates = {}
    for line in stdout.splitlines():
        match = re.search(r"\b([TN]\d{6})\s+.*?\[(?:\d+)\].*?([+-]?\d\.\d{4}e[+-]\d+)\s+([+-]?\d\.\d{4}e[+-]\d+)\s*$", line, re.I)
        if match and match.group(1) != result.key:
            rates[match.group(1)] = float(match.group(2))
    if set(rates) != set(duration) or any(not math.isfinite(rate) or rate <= 0 for rate in rates.values()):
        raise ValueError("Missing or invalid r8s fitted branch rates")
    nodes = [{"id": original.key, "label": original.label, "age": ages[original.key],
              "original_length": original.length, "original_length_text": original.length_text,
              "duration": duration.get(original.key), "fitted_rate_per_site": rates.get(original.key)}
             for original in source_nodes]
    return matches[0], nodes, duration


def approximate_objective(root, nodes, units, sites, penalty, lam):
    """Reevaluate r8s's PL formula from its rounded printed estimates."""
    by_id = {node["id"]: node for node in nodes}
    negative_log_likelihood = 0.0
    roughness = 0.0
    for node in root.walk():
        if node is root:
            continue
        measured = by_id[node.key]
        count = int(Decimal(node.length_text) * (sites if units == "substitutions_per_site" else 1))
        rate = measured["fitted_rate_per_site"] * sites
        expected = rate * measured["duration"]
        if expected <= 0 or not math.isfinite(expected):
            raise ValueError("Nonpositive fitted substitution expectation")
        negative_log_likelihood += expected - count * math.log(expected) + math.lgamma(count + 1)
        if node.children:
            for child in node.children:
                descendant = by_id[child.key]["fitted_rate_per_site"] * sites
                delta = (math.log(rate) - math.log(descendant)) if penalty == "log" else (rate - descendant)
                roughness += delta * delta
    basal = [by_id[child.key]["fitted_rate_per_site"] * sites for child in root.children]
    if penalty == "log":
        basal = [math.log(rate) for rate in basal]
    mean = sum(basal) / len(basal)
    roughness += sum((rate - mean) ** 2 for rate in basal) / len(basal)
    return {"negative_log_likelihood": negative_log_likelihood,
            "roughness": roughness, "penalized_objective": negative_log_likelihood + lam * roughness,
            "precision": "approximate, from rounded r8s output"}
