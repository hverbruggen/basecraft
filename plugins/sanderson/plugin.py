"""Schema-2 Basecraft operation for the original-author r8s PL engine."""

import hashlib
import json
from pathlib import Path
import re

from basecraft.plugin import Plugin

from r8s import approximate_objective, engine_identity, execute, parse_cv, parse_fit, script, settings
from tree import labeled_newick, parse

plugin = Plugin("basecraft.sanderson", "0.1.0")


@plugin.context_operation("date_tree")
def date_tree(_snapshot, parameters, context):
    source = Path(context["inputs"]["tree"][0])
    raw = source.read_bytes()
    root = parse(raw.decode("utf-8"))
    options = settings(parameters, root)
    binary, identity = engine_identity(Path(__file__).resolve().parent)
    cancel = context.get("cancel_file")
    cv_rows = None
    cv_stdout = None
    selected = options["lambda"]
    if options["cross_validation"] is not None:
        cv_stdout, _ = execute(binary, script(root, options, cv=True), cancel)
        cv_rows, selected = parse_cv(cv_stdout, options["cross_validation"])
    fit_stdout, fit_stderr = execute(binary, script(root, options, lam=selected), cancel)
    _engine_tree, nodes, durations = parse_fit(fit_stdout, root, options["root_age"], options["constraints"])
    chronogram = labeled_newick(root, durations) + ";\n"
    objective = approximate_objective(root, nodes, options["units"], options["sites"], options["penalty"], selected)
    best_start = int(re.search(r"Using optimization from starting point\s+(\d+)\s+as best estimate", fit_stdout).group(1))
    objective_line = re.search(r"(?m)^Obj->\s+([^\n]+)", fit_stdout)
    if objective_line:
        printed = [float(value) for value in objective_line.group(1).split()]
        if len(printed) >= best_start:
            objective["engine_reported_objective"] = printed[best_start - 1]
            objective["difference_from_printed"] = objective["penalized_objective"] - printed[best_start - 1]
    report = {
        "schema": 1,
        "method": "Sanderson penalized likelihood",
        "engine": identity,
        "source_sha256": hashlib.sha256(raw).hexdigest(),
        "source_newick": raw.decode("utf-8"),
        "requested_parameters": parameters,
        "effective_settings": {**options, "lambda": selected, "algorithm": "r8s truncated Newton",
                               "blformat_round": False, "pl_forces_integer_counts": True},
        "cross_validation": None if cv_rows is None else {
            "method": "r8s terminal-branch prediction; minimum chi-square error",
            "grid": cv_rows, "selected_lambda": selected,
            "printed_score_precision": "two decimal places", "engine_output": cv_stdout,
        },
        "nodes": nodes,
        "objective_recheck": objective,
        "diagnostics": {
            "engine_stdout": fit_stdout, "engine_stderr": fit_stderr,
            "chronogram_duration_precision": "six decimal places",
            "estimated_rate_precision": "five significant figures",
            "status": "parsed; topology, tip age and hard bounds checked",
        },
        "limitations": [
            "r8s reports rounded durations and rates; objective parity needs full-precision engine output",
            "A converged local PL estimate is not biological validation or an uncertainty interval",
        ],
    }
    # These paths are allocated by the shared host. It verifies and publishes
    # the complete result set after this handler returns successfully.
    Path(context["outputs"]["chronogram"]).write_text(chronogram)
    Path(context["outputs"]["report"]).write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    return {"data": {"lambda": selected, "root_age": options["root_age"],
                     "time_unit": options["time_unit"], "node_count": len(nodes),
                     "engine_sha256": identity["binary_sha256"]},
            "artifacts": [{"role": "chronogram", "path": "."}, {"role": "report", "path": "."}]}


if __name__ == "__main__":
    plugin.run()
