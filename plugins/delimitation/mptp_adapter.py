"""Originating-group mPTP wrapper; explicit single/multiple-rate ML only."""
import json
import math
import os
import re
import shutil
import subprocess
import time
from pathlib import Path


def parse_result(text, expected):
    groups = []
    current = None
    for line in text.splitlines():
        line = line.strip()
        if re.fullmatch(r"Species \d+:", line):
            current = []
            groups.append(current)
        elif current is not None and line:
            current.append(line)
    flattened = [name for group in groups for name in group]
    if not groups or any(not group for group in groups) or len(flattened) != len(set(flattened)) or set(flattened) != set(expected):
        raise ValueError("mPTP output has missing, duplicated or unknown tip assignments")
    best = re.search(r"Best score for (single|multi) coalescent rate:\s*([-+\deE.]+)", text)
    null = re.search(r"Null-model score:\s*([-+\deE.]+)", text)
    count = re.search(r"Number of delimited species:\s*(\d+)", text)
    lrt_passed = re.search(r"^LRT: passed$", text, re.MULTILINE) is not None
    if not best or not null:
        raise ValueError("mPTP output is missing likelihood scores")
    score = float(best[2] if lrt_passed else null[1])
    if not math.isfinite(score):
        raise ValueError("Nonfinite mPTP score")
    warnings = []
    if count and int(count[1]) != len(groups):
        warnings.append("mPTP fitted-count header differs from printed assignments after null-model selection; using the printed partition")
    return {"groups": groups, "log_likelihood": score, "fitted_log_likelihood": float(best[2]),
                "null_log_likelihood": float(null[1]), "lrt_passed": lrt_passed, "diagnostics": warnings}


MISSING = "The mPTP engine is missing from this Basecraft installation. Reinstall the complete Basecraft bundle or configure BASECRAFT_MPTP."


def executable_path(params=None):
    explicit = (params or {}).get("mptp_executable") or os.environ.get("BASECRAFT_MPTP")
    if explicit:
        return str(explicit)
    bundled = Path(__file__).resolve().parents[2] / "bin" / ("mptp.exe" if os.name == "nt" else "mptp")
    if bundled.is_file():
        return str(bundled)
    return shutil.which("mptp")


def engine_problem(params=None):
    """Why mPTP cannot run here, or None."""
    executable = executable_path(params)
    if not executable or not Path(executable).is_file():
        return MISSING
    if not os.access(executable, os.X_OK):
        return f"mPTP is not executable: {executable}. Repair the installation or configured engine path."
    try:
        subprocess.run([executable, "--version"], capture_output=True, timeout=10, check=True)
    except (OSError, subprocess.SubprocessError) as error:
        return f"mPTP could not start: {error}. Repair the installation or configured engine path."
    return None


def invoke(method, params, context, run_native):
    """Run PTP or mPTP on the allocated tree, then record the partition
    through the native engine."""
    problem = engine_problem(params)
    if problem:
        raise RuntimeError(problem)
    executable = executable_path(params)
    # Left out, the native step reads the tree's own rooting (D18).
    if params.get("rooted") is False:
        raise ValueError("PTP/mPTP requires a rooted substitution-length tree")
    minimum = params.get("min_branch", 0.0)
    if not isinstance(minimum, (int, float)) or isinstance(minimum, bool) or not math.isfinite(minimum) or minimum < 0:
        raise ValueError("Minimum branch length must be finite and nonnegative")
    snapshot = Path(context.input("tree"))
    target = Path(context.output("artifacts", "result.json"))
    directory = target.with_name(target.name + ".mptp-" + context.run_id)
    directory.mkdir(parents=True)
    run_native("inspect_tree", params, context, snapshot, directory / "input.json")
    expected = json.loads((directory / "input.json").read_text())["metrics"]["tips"]
    output = directory / "result"
    command = [str(executable), "--ml", "--single" if method == "ptp" else "--multi",
               "--tree_file", str(snapshot), "--output_file", str(output),
               "--minbr", str(minimum), "--seed", str(params.get("seed", 1))]
    version = subprocess.check_output([str(executable), "--version"], stderr=subprocess.STDOUT, text=True).strip()
    started = time.monotonic()
    stdout = directory / "stdout.log"
    stderr = directory / "stderr.log"
    with stdout.open("w") as out, stderr.open("w") as err:
        process = subprocess.Popen(command, stdout=out, stderr=err)
        while process.poll() is None:
            if context.cancelled():
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                break
            time.sleep(0.1)
    report = Path(str(target) + ".run.json")
    if process.returncode:
        status = "cancelled" if context.cancelled() else "failed"
        diagnostic = stderr.read_text()[-16000:] or "mPTP stopped without a partition"
        report.write_text(json.dumps({"job": {"id": context.run_id, "generation": params.get("generation", 0),
                          "method": method, "status": status, "inputs": {}, "outputs": {},
                          "diagnostics": [diagnostic], "elapsed_seconds": time.monotonic()-started}, "command": command}, sort_keys=True)+"\n")
        raise RuntimeError(diagnostic)
    parsed = parse_result(Path(str(output) + ".txt").read_text(), expected)
    parameters = dict(params, **parsed, mptp_version=version, command=command,
                      min_branch=minimum, units="substitutions per site")
    return run_native(method, parameters, context, snapshot, target)
