#!/usr/bin/env python3
"""Measure verified workloads and append portable summaries to history.jsonl.

Run under the site's shared build lock. Cargo and temporary outputs stay outside
this checkout; only numeric summaries and user-chosen machine labels are saved.
"""
import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
COMPACT = ("open-alignment", "edit-alignment", "undo-alignment", "save-alignment")
FRAMES = tuple(f"frame-{phase}-{kind}" for kind in
               ("alignment", "alignment-hidden", "genome-linear", "genome-wrapped", "genome-wrapped-hidden")
               for phase in ("first", "warm", "scroll"))
DEFAULT = (*COMPACT, "open-fasta-100mb", *FRAMES, "tree-open-50000", "tree-midpoint-50000")


def external(path):
    path = path.expanduser().resolve()
    if path == ROOT or ROOT in path.parents:
        raise ValueError("build and fixture storage must be outside the source checkout")
    return path


def source_identity():
    # Archive builds have no .git; identify their actual source rather than
    # inventing a commit. Callers can provide the archive's commit explicitly.
    digest = hashlib.sha256()
    for path in sorted(ROOT.glob("crates/**/*.rs")):
        digest.update(str(path.relative_to(ROOT)).encode())
        digest.update(path.read_bytes())
    revision = os.environ.get("BASECRAFT_BENCH_REVISION")
    if not revision and (ROOT / ".git").exists():
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if revision and not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("BASECRAFT_BENCH_REVISION must be a full commit ID")
    return revision, digest.hexdigest()


def summarize(samples):
    if not samples or any(not s.get("verified") for s in samples):
        raise ValueError("workload verification failed")
    names = {s["scenario"] for s in samples}
    if len(names) != 1:
        raise ValueError("mixed scenarios")
    values = [s["duration_ns"] / s["operations"] / 1e6 for s in samples]
    if any(value <= 0 for value in values):
        raise ValueError("invalid timing")
    return statistics.median(values)


def ceiling(name, budgets):
    if name.startswith("frame-"):
        return budgets["limits_ms"]["frame"]
    return budgets["limits_ms"][name]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--machine", default=os.environ.get("BASECRAFT_BENCH_MACHINE", "unspecified"),
                        help="neutral label, never a hostname")
    parser.add_argument("--cache", type=Path, default=Path(os.environ.get("BASECRAFT_CACHE_DIR", Path.home() / ".cache/basecraft")) / "benchmarks")
    parser.add_argument("--compile-lock", type=Path, default=os.environ.get("BASECRAFT_COMPILE_LOCK"),
                        help="shared host build lock; defaults to the benchmark cache lock")
    parser.add_argument("--history", type=Path, default=ROOT / "benches/history.jsonl")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--compact", action="store_true")
    parser.add_argument("--large", action="store_true")
    parser.add_argument("--scenario", action="append", choices=(*DEFAULT, "open-fasta-1gb"))
    parser.add_argument("--gate", choices=("gross", "benchmark"), default="benchmark")
    args = parser.parse_args()
    if args.samples < 1 or not re.fullmatch(r"[A-Za-z0-9_-]{1,40}", args.machine):
        parser.error("need positive sample count and a neutral alphanumeric machine label")
    selected = list(args.scenario or (COMPACT if args.compact else DEFAULT))
    if args.large:
        selected.append("open-fasta-1gb")
    budgets = tomllib.loads((ROOT / "benches/budgets.toml").read_text())
    previous = [json.loads(line) for line in args.history.read_text().splitlines() if line.strip()] if args.history.exists() else []
    cache = external(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    lock_path = external(args.compile_lock or cache / "compile.lock")
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock = lock_path.open("a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as error:
        raise RuntimeError("another build owns the shared compile lock; timing samples refused") from error
    commit, source_hash = source_identity()
    env = dict(os.environ, BASECRAFT_BENCH_REVISION=commit or source_hash[:40])
    env["CARGO_TARGET_DIR"] = str(external(Path(env.get("CARGO_TARGET_DIR", cache / "target"))))
    env["TMPDIR"] = str(external(Path(env.get("TMPDIR", cache / "tmp"))))
    Path(env["TMPDIR"]).mkdir(parents=True, exist_ok=True)
    env["CARGO_BUILD_JOBS"] = str(min(24, int(env.get("CARGO_BUILD_JOBS", "12"))))
    build = subprocess.run(["cargo", "build", "--locked", "--offline", "--release", "-p", "basecraft-ops",
                            "--bench", "interaction", "--message-format=json"], cwd=ROOT, env=env,
                           text=True, stdout=subprocess.PIPE, check=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    executable = next(a["executable"] for a in artifacts if a.get("executable") and a.get("target", {}).get("name") == "interaction")
    failed = False
    run = Path(tempfile.mkdtemp(prefix="interaction-", dir=cache))
    for name in selected:
        samples = []
        for index in range(args.samples):
            work = run / f"{name}-{index}"
            work.mkdir()
            child = dict(env, BASECRAFT_PROFILE_DIR=str(work / "profile"),
                         BASECRAFT_STATE_DIR=str(work / "profile/state"), BASECRAFT_CACHE_DIR=str(work / "profile/cache"),
                         BASECRAFT_SCRATCH_DIR=str(work), TMPDIR=str(work), TMP=str(work), TEMP=str(work))
            result = subprocess.run([executable, name, str(work)], env=child, text=True, capture_output=True)
            (run / f"{name}-{index}.log").write_text(result.stdout + result.stderr)
            if result.returncode:
                raise RuntimeError(f"{name} failed; see {run / f'{name}-{index}.log'}")
            samples.append(json.loads(result.stdout))
            shutil.rmtree(work)  # Only this run's disposable fixtures.
        median = summarize(samples)
        baseline = next((row["median_ms"] for row in reversed(previous)
                         if row.get("machine") == args.machine and row.get("scenario") == name
                         and row.get("protocol") == "interaction-v2" and row.get("passed")), None)
        ratio = budgets["comparison"]["gross_ci_ratio" if args.gate == "gross" else "benchmark_ratio"]
        limit = ceiling(name, budgets) * (2 if args.gate == "gross" else 1)
        if baseline is not None and args.machine != "unspecified":
            limit = baseline * ratio
        passed = median <= limit
        row = dict(date=datetime.now(timezone.utc).isoformat(), commit=commit, source_sha256=source_hash,
                   scenario=name, median_ms=median, samples=len(samples), machine=args.machine,
                   protocol="interaction-v2", limit_ms=limit, passed=passed)
        args.history.parent.mkdir(parents=True, exist_ok=True)
        with args.history.open("a") as stream:
            stream.write(json.dumps(row, sort_keys=True) + "\n")
        print(f"{name}: {median:.3f} ms (limit {limit:g}) {'PASS' if passed else 'FAIL'}", flush=True)
        failed |= not passed
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
