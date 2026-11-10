#!/usr/bin/env python3
"""Build/run isolated warm-cache core measurements; all output stays in explicit cache."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SCENARIOS = ("open-alignment", "open-fasta-100mb", "edit-alignment",
             "undo-alignment", "save-alignment")
FRAME_SCENARIOS = tuple(f"frame-{phase}-{case}"
                        for case in ("alignment", "alignment-hidden", "genome-linear",
                                     "genome-wrapped", "genome-wrapped-hidden")
                        for phase in ("first", "warm", "scroll"))
TREE_SCENARIOS = ("tree-open-50000", "tree-midpoint-50000")
PROTOCOL = "session-warm-v1"
SCHEMA_VERSION = 2


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def external(path):
    path = path.expanduser().resolve()
    if path == ROOT or ROOT in path.parents:
        raise ValueError("benchmark output/build storage must be outside the source checkout")
    return path


def summarize(samples):
    if not samples or any(not s.get("verified") for s in samples):
        raise ValueError("missing or unverified measurements")
    if any(s.get("schema_version") != SCHEMA_VERSION for s in samples):
        raise ValueError("unsupported sample schema; regenerate rather than reinterpret old fixture sizes")
    keys = ("scenario", "operations", "fixture_bytes", "rows", "columns", "edits",
            "source_commit", "debug_assertions")
    keys += ("work",) if samples[0]["scenario"].startswith(("frame-", "tree-")) else ()
    identity = {key: samples[0][key] for key in keys}
    if any({key: s[key] for key in keys} != identity for s in samples):
        raise ValueError("inconsistent sample parameters")
    if identity["operations"] <= 0 or any(s["duration_ns"] <= 0 for s in samples):
        raise ValueError("invalid duration or operation count")
    if samples[0]["scenario"].startswith("frame-") and any(
            len(s.get("output_counts", [])) != s["operations"]
            or any(n <= 0 for n in s["output_counts"]) for s in samples):
        raise ValueError("missing frame output verification counts")
    return {**identity, "status": "measured", "samples": len(samples),
            "unit": "ns/operation", "median": statistics.median(
                s["duration_ns"] / s["operations"] for s in samples)}


def compare(report, baseline, budgets):
    # Compare only compatible runner/fixture/machine identities. No baseline is a pass.
    if baseline is None:
        return {"status": "unavailable", "reason": "no comparison baseline supplied"}
    for key in ("schema_version", "machine", "profile", "protocol", "runner_sha256"):
        if baseline.get(key) != report[key]:
            return {"status": "unavailable", "reason": f"baseline {key} differs"}
    results = {}
    for name, current in report["scenarios"].items():
        previous = baseline.get("scenarios", {}).get(name, {})
        if current.get("status") != "measured" or previous.get("status") != "measured":
            results[name] = {"status": "unavailable", "reason": "scenario not measured in both runs"}
            continue
        keys = ("operations", "fixture_bytes", "rows", "columns", "edits", "unit")
        keys += ("work",) if name.startswith(("frame-", "tree-")) else ()
        median = previous.get("median", 0)
        if (any(current[k] != previous.get(k) for k in keys)
                or not isinstance(median, (int, float)) or not math.isfinite(median) or median <= 0):
            results[name] = {"status": "unavailable", "reason": "incompatible fixture or median"}
            continue
        ratio = current["median"] / median
        results[name] = {"status": "compared", "ratio": ratio,
                         "over_30_percent": ratio > budgets["benchmark_ratio"],
                         "over_2x": ratio > budgets["gross_ci_ratio"]}
    return {"status": "observational", "scenarios": results, "gate_enabled": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--machine", required=True, help="user-chosen neutral label, never an actual hostname")
    parser.add_argument("--cache", required=True, type=Path, help="explicit external output and fixture cache")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--baseline", type=Path, help="reviewed report.json from a compatible run")
    parser.add_argument("--history", type=Path, help="opt-in external JSONL summary history (review before publishing)")
    parser.add_argument("--scenario", action="append", choices=SCENARIOS + FRAME_SCENARIOS + TREE_SCENARIOS,
                        help="measure selected Session or frame scenarios (repeatable)")
    parser.add_argument("--large", action="store_true", help="also open 1 GB decimal FASTA (may refuse)")
    parser.add_argument("--executable", type=Path, help="reuse a release bench binary built with this source revision")
    parser.add_argument("--revision", help="exact archive revision when .git is absent")
    args = parser.parse_args()
    if args.samples < 3 or not re.fullmatch(r"[A-Za-z0-9_-]{1,40}", args.machine):
        parser.error("need at least 3 samples and a neutral alphanumeric machine label")
    cache = external(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    revision = args.revision
    if (ROOT / ".git").exists():
        if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT).strip():
            parser.error("commit source changes before exact-source measurement")
        actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        if revision and revision != actual:
            parser.error("revision differs from checkout")
        revision = actual
    if not revision or not re.fullmatch(r"[a-f0-9]{40}", revision):
        parser.error("an exact source revision is required")
    history = external(args.history) if args.history else None
    budgets = tomllib.loads((ROOT / "benches/budgets.toml").read_text())
    runner_hash = hashlib.sha256(b"".join((ROOT / name).read_bytes() for name in (
        "benches/run.py", "crates/basecraft-ops/benches/interaction.rs",
        "crates/basecraft-ops/benches/interaction/frame_cases.rs",
        "crates/basecraft-ops/benches/interaction/tree_cases.rs", "benches/budgets.toml",
        "crates/basecraft-ops/Cargo.toml"))).hexdigest()
    run_dir = Path(tempfile.mkdtemp(prefix="interaction-", dir=cache))
    env = os.environ.copy()
    env.update(BASECRAFT_BENCH_REVISION=revision)
    if args.executable:
        executable = args.executable.resolve()
    else:
        # Caller must acquire its site's shared compilation lock for this entire command.
        # No host names, SSH defaults, or machine-specific lock paths belong here.
        env["CARGO_TARGET_DIR"] = str(external(Path(env.get("CARGO_TARGET_DIR", cache / "target"))))
        compiler_tmp = external(Path(env.get("TMPDIR", cache / "tmp")))
        compiler_tmp.mkdir(parents=True, exist_ok=True)
        env["TMPDIR"] = str(compiler_tmp)
        env["CARGO_BUILD_JOBS"] = str(min(24, int(env.get("CARGO_BUILD_JOBS", "24"))))
        build = subprocess.run(["cargo", "build", "--locked", "--offline", "--release",
                                "-p", "basecraft-ops", "--bench", "interaction", "--message-format=json"],
                               cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, check=True)
        artifacts = [json.loads(line) for line in build.stdout.splitlines()]
        executable = Path(next(a["executable"] for a in artifacts if a.get("executable")
                               and a.get("target", {}).get("name") == "interaction"))
    report = {"schema_version": SCHEMA_VERSION, "source_commit": revision, "profile": "release",
              "machine": args.machine, "protocol": PROTOCOL, "runner_sha256": runner_hash,
              "executable_sha256": digest(executable), "date_utc": datetime.now(timezone.utc).isoformat(),
              "cache": "Session: warm OS/index cache; frame: cold view for first, warmed view for warm/scroll; see scenario work",
              "scenarios": {}}
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    failed = False
    for name in (*(args.scenario or SCENARIOS), *(("open-fasta-1gb",) if args.large else ())):
        samples = []
        for index in range(args.samples):
            work = run_dir / f"{name}-{index}"
            work.mkdir()
            child_env = env.copy()
            child_env.update(BASECRAFT_PROFILE_DIR=str(work / "profile"),
                             BASECRAFT_STATE_DIR=str(work / "profile/state"),
                             BASECRAFT_CACHE_DIR=str(work / "profile/cache"),
                             BASECRAFT_SCRATCH_DIR=str(work), TMPDIR=str(work), TMP=str(work), TEMP=str(work))
            result = subprocess.run([str(executable), name, str(work)], env=child_env,
                                    text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            (work / "stdout.txt").write_text(result.stdout)
            (work / "stderr.txt").write_text(result.stderr)
            if result.returncode:
                failed = True
                report["scenarios"][name] = {"status": "failed_or_refused", "sample": index,
                                             "exit_code": result.returncode, "completed_samples": len(samples)}
                print(f"{name}: failed/refused; see {work / 'stderr.txt'}", file=sys.stderr)
                break
            sample = json.loads(result.stdout)
            if sample["source_commit"] != revision or sample["debug_assertions"] or sample["scenario"] != name:
                raise ValueError("binary identity/profile/scenario mismatch")
            samples.append(sample)
            # Remove only fixtures/profile belonging to this sample; preserve raw logs.
            import shutil
            for item in work.iterdir():
                if item.name not in ("stdout.txt", "stderr.txt"):
                    if item.is_dir():
                        shutil.rmtree(item)
                    else:
                        item.unlink()
        else:
            report["scenarios"][name] = summarize(samples)
            print(f"{name}: {report['scenarios'][name]['median'] / 1e6:.3f} ms/operation ({len(samples)} samples)", flush=True)
    report["comparison"] = compare(report, baseline, budgets["comparison"])
    report_path = run_dir / "report.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    if history:
        history.parent.mkdir(parents=True, exist_ok=True)
        with history.open("a") as stream:
            stream.write(json.dumps(report) + "\n")
    print(f"comparison: {report['comparison']['status']}; no timing gate enabled")
    print(f"report: {report_path}")
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        sys.exit(f"benchmark failed: {error}")
