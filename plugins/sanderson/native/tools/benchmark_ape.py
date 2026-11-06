#!/usr/bin/env python3
"""Prepare and score deterministic native PL vs ape::chronos benchmark cases.

All timed runs belong on the build host under the shared lock. This helper does no
compilation. R/ape is a benchmark-only comparator and is never a plugin engine.
"""

import argparse
import csv
import hashlib
import json
import math
import statistics
import subprocess
import time
from pathlib import Path

from generate_fixture import make


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(args):
    nodes, tree = make(args.tips, args.kind)
    root = args.out
    root.mkdir(parents=True)
    (root / "tree.nwk").write_text(tree)
    ages = [None] * len(nodes)
    ages[0] = 1.0
    clades = [None] * len(nodes)
    for i, node in enumerate(nodes):
        if i:
            parent = node["parent"]
            ages[i] = ages[parent] / 2.0 if node["children"] else 0.0
    for i in range(len(nodes) - 1, -1, -1):
        clades[i] = ([nodes[i]["label"]] if not nodes[i]["children"] else
                     sorted(tip for child in nodes[i]["children"] for tip in clades[child]))
    fixed_node = 1 if args.constraint else None
    constraints = [] if fixed_node is None else [{"node": fixed_node,
                                                  "min_age": ages[fixed_node],
                                                  "max_age": ages[fixed_node]}]
    request = {"nodes": nodes, "cancel_file": None,
               "parameters": {"branch_units": "substitutions_per_site", "sites": 1,
                              "root_age": 1.0, "time_unit": "relative units",
                              "penalty": "add", "lambda": args.lambda_value,
                              "starts": args.starts, "seed": 17,
                              "constraints": constraints, "cross_validation": None,
                              "max_iterations": 2000, "max_seconds": 600}}
    (root / "native-request.json").write_text(json.dumps(request, indent=2) + "\n")
    truth = {",".join(clades[i]): {"age": ages[i], "rate": None if i == 0 else
             nodes[i]["length"] / (ages[nodes[i]["parent"]] - ages[i]),
             "length": nodes[i]["length"],
             "parent": None if i == 0 else ",".join(clades[nodes[i]["parent"]]),
             "fixed": i == fixed_node}
             for i in range(len(nodes))}
    (root / "truth.json").write_text(json.dumps(truth, indent=2, sort_keys=True) + "\n")
    (root / "config.tsv").write_text("lambda\tconstraint\tconstraint_age\n"
                                     f"{args.lambda_value}\t{int(args.constraint)}\t"
                                     f"{ages[1] if args.constraint else 0}\n")
    print(json.dumps({"fixture": str(root), "tips": args.tips, "kind": args.kind,
                      "constraint": args.constraint, "tree_sha256": sha256(root / "tree.nwk"),
                      "request_sha256": sha256(root / "native-request.json")}, sort_keys=True))


def native(args):
    request = (args.fixture / "native-request.json").read_bytes()
    results = []
    for rep in range(args.repetitions):
        start = time.perf_counter()
        process = subprocess.run([str(args.binary)], input=request, capture_output=True,
                                 timeout=args.timeout, check=False)
        elapsed = time.perf_counter() - start
        record = {"rep": rep, "elapsed_s": elapsed, "exit_code": process.returncode,
                  "stdout_sha256": hashlib.sha256(process.stdout).hexdigest(),
                  "stderr": process.stderr.decode(errors="replace")[-2000:]}
        if process.returncode == 0:
            record["result"] = json.loads(process.stdout)
        results.append(record)
        if process.returncode:
            break
    args.output.write_text(json.dumps({"binary_sha256": sha256(args.binary),
                                       "fixture_sha256": sha256(args.fixture / "native-request.json"),
                                       "runs": results}, indent=2) + "\n")
    print(json.dumps({"runs": len(results), "successful": sum(x["exit_code"] == 0 for x in results),
                      "median_s": statistics.median(x["elapsed_s"] for x in results)}, sort_keys=True))


def score_fit(truth, result, lambda_value):
    assert set(result) == set(truth), (set(truth) - set(result), set(result) - set(truth))
    ages = [result[k]["age"] - truth[k]["age"] for k in truth]
    rates = [result[k]["rate"] - truth[k]["rate"] for k in truth if truth[k]["rate"] is not None]
    nll = nonbasal = 0.0
    basal = []
    valid = True
    for clade, node in truth.items():
        if node["parent"] is None:
            continue
        age = result[clade]["age"]
        parent_age = result[node["parent"]]["age"]
        duration = parent_age - age
        rate = result[clade]["rate"]
        valid &= math.isfinite(duration) and duration > 0 and math.isfinite(rate) and rate > 0
        if not valid:
            continue
        expectation = duration * rate
        observed = node["length"]
        nll += expectation - observed * math.log(expectation) + math.lgamma(observed + 1)
        if truth[node["parent"]]["parent"] is None:
            basal.append(rate)
        else:
            nonbasal += (rate - result[node["parent"]]["rate"]) ** 2
    assert len(basal) == 2, basal
    root_pop = sum((value - sum(basal) / 2) ** 2 for value in basal) / 2
    root_sample = root_pop * 2
    constraint = next((abs(result[k]["age"] - node["age"]) for k, node in truth.items()
                       if node.get("fixed")), 0.0)
    return {"valid_positive_durations_rates": valid,
            "age_rmse": math.sqrt(statistics.mean(x * x for x in ages)),
            "age_max_abs": max(map(abs, ages)),
            "rate_rmse": math.sqrt(statistics.mean(x * x for x in rates)),
            "rate_max_abs": max(map(abs, rates)),
            "likelihood_nll": nll, "nonbasal_roughness": nonbasal,
            "root_population_variance": root_pop, "root_sample_variance": root_sample,
            "native_objective_at_fit": nll + lambda_value * (nonbasal + root_pop),
            "ape_source_objective_at_fit": nll + lambda_value * (nonbasal - root_sample),
            "fixed_age_error": constraint}


def score(args):
    fixture = args.fixture
    truth = json.loads((fixture / "truth.json").read_text())
    config = next(csv.DictReader((fixture / "config.tsv").open(), delimiter="\t"))
    lam = float(config["lambda"])
    raw_native = json.loads(args.native.read_text())
    native_runs = []
    nodes = json.loads((fixture / "native-request.json").read_text())["nodes"]
    keys = [None] * len(nodes)
    def clade(i):
        if keys[i] is None:
            keys[i] = nodes[i]["label"] if not nodes[i]["children"] else ",".join(sorted(
                tip for child in nodes[i]["children"] for tip in clade(child).split(",")))
        return keys[i]
    clade(0)
    for run in raw_native["runs"]:
        if run["exit_code"]:
            native_runs.append({"elapsed_s": run["elapsed_s"], "error": run["stderr"]})
            continue
        out = run["result"]
        fit = {keys[i]: {"age": out["ages"][i], "rate": out["rates_per_site"][i]}
               for i in range(len(nodes))}
        independent = score_fit(truth, fit, lam)
        independent.update(elapsed_s=run["elapsed_s"], reported_objective=out["objective"],
                           gradient_norm=out["gradient_norm"],
                           accepted_starts=sum("rejected" not in x["termination"]
                                               for x in out["starts"]),
                           objective_discrepancy=out["objective"] -
                           independent["native_objective_at_fit"])
        native_runs.append(independent)
    ape_runs = []
    for meta_path in sorted(args.ape.glob("meta-*.tsv")):
        meta = next(csv.DictReader(meta_path.open(), delimiter="\t"))
        fit_path = args.ape / meta_path.name.replace("meta-", "fit-")
        if not fit_path.exists():
            ape_runs.append({"elapsed_s": float(meta["elapsed_s"]), "error": meta["message"]})
            continue
        fit = {row["clade"]: {"age": float(row["age"]), "rate": float(row["rate"])}
               for row in csv.DictReader(fit_path.open(), delimiter="\t")}
        root = next(k for k, v in truth.items() if v["parent"] is None)
        fit[root] = {"age": 1.0, "rate": None}
        independent = score_fit(truth, fit, lam)
        independent.update(elapsed_s=float(meta["elapsed_s"]),
                           reported_ploglik=float(meta["ploglik"]),
                           convergence=meta["convergence"] == "TRUE",
                           message=meta["message"], warnings=meta["warnings"],
                           niter=int(meta["niter"]),
                           objective_discrepancy=-float(meta["ploglik"]) -
                           independent["ape_source_objective_at_fit"])
        ape_runs.append(independent)
    report = {"fixture": str(fixture), "tree_sha256": sha256(fixture / "tree.nwk"),
              "lambda": lam, "native": native_runs, "ape": ape_runs}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    for name, runs in (("native", native_runs), ("ape", ape_runs)):
        valid = [r for r in runs if "error" not in r]
        print(json.dumps({"method": name, "runs": len(runs), "valid": len(valid),
                          "converged": sum(r.get("convergence", r.get("accepted_starts", 0) > 0)
                                           for r in valid),
                          "median_elapsed_s": statistics.median(r["elapsed_s"] for r in valid)
                          if valid else None,
                          "age_rmse": valid[0]["age_rmse"] if valid else None,
                          "rate_rmse": valid[0]["rate_rmse"] if valid else None}, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("--out", type=Path, required=True)
    prep.add_argument("--tips", type=int, required=True)
    prep.add_argument("--kind", choices=("clock", "nonclock"), required=True)
    prep.add_argument("--constraint", action="store_true")
    prep.add_argument("--lambda", dest="lambda_value", type=float, default=1.0)
    prep.add_argument("--starts", type=int, default=3)
    run = sub.add_parser("native-run")
    run.add_argument("--fixture", type=Path, required=True)
    run.add_argument("--binary", type=Path, required=True)
    run.add_argument("--repetitions", type=int, default=10)
    run.add_argument("--timeout", type=float, default=600)
    run.add_argument("--output", type=Path, required=True)
    finish = sub.add_parser("score")
    finish.add_argument("--fixture", type=Path, required=True)
    finish.add_argument("--native", type=Path, required=True)
    finish.add_argument("--ape", type=Path, required=True)
    finish.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    {"prepare": prepare, "native-run": native, "score": score}[args.action](args)


if __name__ == "__main__":
    main()
