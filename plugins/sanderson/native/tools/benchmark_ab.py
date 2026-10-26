#!/usr/bin/env python3
"""Paired, alternating-order native binary timing on one fixed request.

Run on build-host under the shared lock. Each pair reverses process order; the
output must be byte-identical, so timing cannot mask a numerical difference.
"""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import time


def run(binary, request, timeout):
    started = time.perf_counter_ns()
    result = subprocess.run([str(binary)], input=request, capture_output=True,
                            timeout=timeout, check=False)
    elapsed = (time.perf_counter_ns() - started) / 1e9
    if result.returncode:
        raise RuntimeError(f"{binary} failed: {result.stderr[-1000:]!r}")
    return elapsed, result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--request", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=50)
    parser.add_argument("--warmup", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    request = args.request.read_bytes()
    for _ in range(args.warmup):
        _, a_output = run(args.a, request, args.timeout)
        _, b_output = run(args.b, request, args.timeout)
        if a_output != b_output:
            raise AssertionError("native outputs differ during warmup")
    pairs = []
    for index in range(args.repetitions):
        order = ("a", "b") if index % 2 == 0 else ("b", "a")
        sample = {}
        for name in order:
            sample[name], output = run(getattr(args, name), request, args.timeout)
            sample[name + "_sha256"] = hashlib.sha256(output).hexdigest()
        if sample["a_sha256"] != sample["b_sha256"]:
            raise AssertionError(f"native outputs differ at pair {index}")
        sample["order"] = "".join(order)
        pairs.append(sample)
    a_times = [item["a"] for item in pairs]
    b_times = [item["b"] for item in pairs]
    summary = {"a_binary_sha256": hashlib.sha256(args.a.read_bytes()).hexdigest(),
               "b_binary_sha256": hashlib.sha256(args.b.read_bytes()).hexdigest(),
               "request_sha256": hashlib.sha256(request).hexdigest(),
               "pairs": len(pairs), "a_median_s": statistics.median(a_times),
               "b_median_s": statistics.median(b_times),
               "paired_median_a_over_b": statistics.median(a / b for a, b in zip(a_times, b_times)),
               "b_faster_pairs": sum(b < a for a, b in zip(a_times, b_times)),
               "output_sha256": pairs[0]["a_sha256"]}
    args.output.write_text(json.dumps({"summary": summary, "measurements": pairs}, indent=2) + "\n")
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
