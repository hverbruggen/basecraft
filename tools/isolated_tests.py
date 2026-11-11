#!/usr/bin/env python3
"""Run every workspace test in its own process with a fresh Basecraft profile.

Tests register analysis stores in the profile. A shared profile lets one test's
deleted temporary store change another test's save checks, so each test gets
its own profile, state and cache folders. Arguments select cargo test targets
(default: the whole workspace).
"""
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def executables(extra):
    selection = extra or ["--workspace"]
    build = subprocess.run(["cargo", "test", "--no-run", "--message-format=json", *selection],
                           cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    for row in map(json.loads, build.stdout.splitlines()):
        if row.get("reason") == "compiler-artifact" and row.get("executable") and row["profile"]["test"]:
            yield Path(row["executable"]), Path(row["manifest_path"]).parent


def cases(executable, crate):
    listed = subprocess.run([executable, "--list", "--format", "terse"], cwd=crate, text=True,
                            capture_output=True)
    names = [line.removesuffix(": test") for line in listed.stdout.splitlines() if line.endswith(": test")]
    # Custom harnesses do not list cases; run the whole binary once.
    return [(executable, crate, name) for name in names] if listed.returncode == 0 and names \
        else [(executable, crate, None)]


def run(case):
    executable, crate, name = case
    with tempfile.TemporaryDirectory(prefix="basecraft-test-") as profile:
        env = dict(os.environ, BASECRAFT_PROFILE_DIR=profile, BASECRAFT_STATE_DIR=f"{profile}/state",
                   BASECRAFT_CACHE_DIR=f"{profile}/cache")
        args = [executable] + (["--exact", name] if name else [])
        result = subprocess.run(args, cwd=crate, env=env, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT)
    return case, result.returncode, result.stdout


def main():
    all_cases = [case for binary in executables(sys.argv[1:]) for case in cases(*binary)]
    failed = []
    # Timing-sensitive lock tests need the CPU share the container really has.
    workers = int(os.environ.get("CARGO_BUILD_JOBS") or os.cpu_count() or 4)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for (executable, _, name), code, output in pool.map(run, all_cases):
            if code:
                failed.append(f"{executable.name} {name or '(whole binary)'}")
                print(output, flush=True)
    print(f"{len(all_cases) - len(failed)} passed, {len(failed)} failed")
    for line in failed:
        print("FAILED", line)
    return int(bool(failed))


if __name__ == "__main__":
    sys.exit(main())
