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
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from startup import startup_samples
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


def build_binary(package, kind, name, env):
    result = subprocess.run(["cargo", "build", "--locked", "--offline", "--release", "-p", package,
                             kind, name, "--message-format=json"], cwd=ROOT, env=env,
                            text=True, stdout=subprocess.PIPE, check=True)
    return next(row["executable"] for row in map(json.loads, result.stdout.splitlines())
                if row.get("executable") and row.get("target", {}).get("name") == name)


def record(name, values, unit, protocol, args, budgets, commit, source_hash, **details):
    median = statistics.median(values)
    field = "median_ms" if unit == "ms" else "median_bytes"
    limit = ceiling(name, budgets) if unit == "ms" else budgets["limits_bytes"][name]
    limit *= 2 if args.gate == "gross" else 1
    passed = median <= limit
    row = dict(date=datetime.now(timezone.utc).isoformat(), commit=commit, source_sha256=source_hash,
               scenario=name, samples=len(values), machine=args.machine, protocol=protocol,
               unit=unit, passed=passed, **{field: median, "limit_" + unit: limit},
               values=values, **details)
    if args.record:
        args.history.parent.mkdir(parents=True, exist_ok=True)
        with args.history.open("a") as stream:
            stream.write(json.dumps(row, sort_keys=True) + "\n")
    print(f"{name}: {median:.3f} {unit} (limit {limit:g}) {'PASS' if passed else 'FAIL'}", flush=True)
    return passed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--machine", default=os.environ.get("BASECRAFT_BENCH_MACHINE", "unspecified"),
                        help="neutral label, never a hostname")
    parser.add_argument("--cache", type=Path, default=Path(os.environ.get("BASECRAFT_CACHE_DIR", Path.home() / ".cache/basecraft")) / "benchmarks")
    parser.add_argument("--compile-lock", type=Path, default=os.environ.get("BASECRAFT_COMPILE_LOCK"),
                        help="shared host build lock; defaults to the benchmark cache lock")
    parser.add_argument("--history", type=Path, default=ROOT / "benches/history.jsonl")
    parser.add_argument("--record", action="store_true", help="append measured results to history")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--compact", action="store_true")
    parser.add_argument("--large", action="store_true")
    parser.add_argument("--cold", action="store_true", help="also evict and measure selected large FASTA inputs")
    parser.add_argument("--startup", action="store_true", help="measure the real software-rendered offscreen application")
    parser.add_argument("--scenario", action="append", choices=(*DEFAULT, "open-fasta-1gb"))
    parser.add_argument("--gate", choices=("gross", "benchmark"), default="benchmark")
    args = parser.parse_args()
    if args.samples < 1 or not re.fullmatch(r"[A-Za-z0-9_-]{1,40}", args.machine):
        parser.error("need positive sample count and a neutral alphanumeric machine label")
    selected = list(args.scenario or (COMPACT if args.compact else DEFAULT))
    if args.large:
        selected.append("open-fasta-1gb")
    if args.cold:
        selected.append("open-fasta-100mb")
        selected += [name + "-cold" for name in selected if name.startswith("open-fasta-")]
    selected = list(dict.fromkeys(selected))
    budgets = tomllib.loads((ROOT / "benches/budgets.toml").read_text())
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
    executable = build_binary("basecraft-ops", "--bench", "interaction", env)
    gui = build_binary("basecraft-gui", "--bin", "basecraft-gui", env) if args.startup else None
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
        summarize(samples)  # Reject unverified, mixed or invalid samples before writing history.
        values = [sample["duration_ns"] / sample["operations"] / 1e6 for sample in samples]
        details = {}
        if "storage_read_bytes" in samples[0]:
            details["storage_read_bytes"] = [sample["storage_read_bytes"] for sample in samples]
        protocol = "interaction-cold-v1" if name.endswith("-cold") else "interaction-v2"
        failed |= not record(name, values, "ms", protocol, args, budgets,
                             commit, source_hash, **details)
    if gui:
        result = startup_samples(gui, run / "startup", "software", count=args.samples,
                                 cold=True, validation=False)
        (run / "startup.json").write_text(json.dumps(result, indent=2) + "\n")
        for name, key, unit in [("startup-offscreen", "spawn_to_first_frame_ms", "ms"),
                                ("idle-rss-offscreen", "idle_rss_bytes", "bytes")]:
            failed |= not record(name, [sample[key] for sample in result["samples"]], unit,
                                 "offscreen-startup-v1", args, budgets, commit, source_hash,
                                 cache=result["cache"], clock=result["clock"], idle=result["idle"],
                                 backend="software", trace_samples=result["samples"])
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
