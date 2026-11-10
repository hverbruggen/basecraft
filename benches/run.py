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
HEAP_SCENARIOS = ("heap-open-compact", "heap-open-100mb", "heap-open-error")
PROTOCOL = "session-warm-v1"
SCHEMA_VERSION = 2
COMPACT_SCENARIOS = tuple(n for n in SCENARIOS if n != "open-fasta-100mb")


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
    keys += ("work",) if samples[0]["scenario"].startswith(("frame-", "tree-", "heap-")) else ()
    identity = {key: samples[0][key] for key in keys}
    if any({key: s[key] for key in keys} != identity for s in samples):
        raise ValueError("inconsistent sample parameters")
    if identity["scenario"].startswith("heap-"):
        fields = ("baseline_bytes", "peak_bytes", "incremental_peak_bytes", "live_after_open_bytes",
                  "live_after_drop_bytes", "successful_reallocations", "failed_reallocations", "failed_allocations")
        for sample in samples:
            heap = sample["heap"]
            if (any(type(heap.get(k)) is not int or heap[k] < 0 for k in fields)
                    or heap["peak_bytes"] < max(heap["baseline_bytes"], heap["live_after_open_bytes"])
                    or heap["incremental_peak_bytes"] != heap["peak_bytes"] - heap["baseline_bytes"]
                    or heap["failed_reallocations"] or heap["failed_allocations"]
                    or sample["work"].get("instrumentation") != "system-requested-live-v1"
                    or sample["work"].get("unit") != "requested bytes"
                    or identity["operations"] != 1):
                raise ValueError("invalid requested-heap accounting")
        return {**identity, "status": "measured", "samples": len(samples),
                "unit": "requested bytes", "median": statistics.median(s["heap"]["incremental_peak_bytes"] for s in samples),
                "heap_medians": {k: statistics.median(s["heap"][k] for s in samples) for k in fields}}
    if (type(identity["operations"]) is not int or identity["operations"] <= 0
            or any(type(s["duration_ns"]) is not int or s["duration_ns"] <= 0 for s in samples)):
        raise ValueError("invalid duration or operation count")
    if samples[0]["scenario"].startswith("frame-") and any(
            len(s.get("output_counts", [])) != s["operations"]
            or any(n <= 0 for n in s["output_counts"]) for s in samples):
        raise ValueError("missing frame output verification counts")
    values = [s["duration_ns"] / s["operations"] for s in samples]
    median = statistics.median(values)
    return {**identity, "status": "measured", "samples": len(samples),
            "unit": "ns/operation", "median": median, "minimum": min(values),
            "maximum": max(values), "mad": statistics.median(abs(v - median) for v in values)}


def compare(report, baseline, budgets):
    # Compare only compatible runner/fixture/machine identities. No baseline is a pass.
    if baseline is None:
        return {"status": "unavailable", "reason": "no comparison baseline supplied"}
    for key in ("schema_version", "machine", "profile", "protocol", "runner_sha256", "environment_sha256"):
        if baseline.get(key) != report.get(key):
            return {"status": "unavailable", "reason": f"baseline {key} differs"}
    results = {}
    for name, current in report["scenarios"].items():
        previous = baseline.get("scenarios", {}).get(name, {})
        if current.get("status") != "measured" or previous.get("status") != "measured":
            results[name] = {"status": "unavailable", "reason": "scenario not measured in both runs"}
            continue
        keys = ("operations", "fixture_bytes", "rows", "columns", "edits", "unit")
        keys += ("work",) if name.startswith(("frame-", "tree-", "heap-")) else ()
        median = previous.get("median", 0)
        if (any(current[k] != previous.get(k) for k in keys)
                or any(type(value) not in (int, float) or not math.isfinite(value) or value <= 0
                       for value in (median, current.get("median")))):
            results[name] = {"status": "unavailable", "reason": "incompatible fixture or median"}
            continue
        ratio = current["median"] / median
        results[name] = {"status": "compared", "ratio": ratio,
                         "over_30_percent": ratio > budgets["benchmark_ratio"],
                         "over_2x": ratio > budgets["gross_ci_ratio"]}
    return {"status": "observational", "scenarios": results, "gate_enabled": False}


def timing_gate(report, baseline, budgets, mode):
    """Fail closed: all compact workloads and all selected comparisons are required."""
    threshold = budgets["gross_ci_ratio" if mode == "gross" else "benchmark_ratio"]
    errors = []
    for label, candidate in (("current", report), ("baseline", baseline or {})):
        if not re.fullmatch(r"[a-f0-9]{64}", candidate.get("environment_sha256", "")):
            errors.append(f"{label}: missing environment identity")
        for key in ("machine", "runner_sha256", "source_commit", "executable_sha256"):
            if not isinstance(candidate.get(key), str) or not candidate[key]:
                errors.append(f"{label}: missing {key}")
        if (candidate.get("schema_version") != SCHEMA_VERSION
                or candidate.get("profile") != "release" or candidate.get("protocol") != PROTOCOL):
            errors.append(f"{label}: unsupported schema/profile/protocol")
        for name in COMPACT_SCENARIOS:
            sample = candidate.get("scenarios", {}).get(name, {})
            if (sample.get("status") != "measured" or sample.get("unit") != "ns/operation"
                    or type(sample.get("samples")) is not int
                    or sample["samples"] < budgets["minimum_gate_samples"]):
                errors.append(f"{label}: {name} requires verified timing and at least "
                              f"{budgets['minimum_gate_samples']} samples")
    comparison = compare(report, baseline, budgets)
    if comparison["status"] == "unavailable":
        errors.append(comparison["reason"])
    else:
        for name, result in comparison["scenarios"].items():
            if result["status"] != "compared":
                errors.append(f"{name}: {result['reason']}")
            elif result["ratio"] > threshold:
                errors.append(f"{name}: ratio {result['ratio']:.6g} exceeds {threshold:g}")
    return {"status": "failed" if errors else "passed", "gate_enabled": True,
            "mode": mode, "threshold": threshold, "errors": errors}


def environment_identity(path):
    return descriptor_identity(json_object(path, "environment descriptor"))


def descriptor_identity(descriptor):
    # Private, reviewed site descriptor: hardware, toolchain, runtime, storage,
    # build flags and lock/isolation policy. Hash values, never publish its paths.
    required = ("hardware", "toolchain", "runtime", "storage", "build", "isolation")
    if not isinstance(descriptor, dict) or any(
            not isinstance(descriptor.get(k), str) or not descriptor[k].strip() for k in required):
        raise ValueError("environment descriptor requires nonempty " + ", ".join(required))
    return hashlib.sha256(json.dumps(descriptor, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


RUNNER_INPUTS = (
    "benches/run.py", "crates/basecraft-ops/benches/interaction.rs",
    "crates/basecraft-ops/benches/interaction/frame_cases.rs",
    "crates/basecraft-ops/benches/interaction/tree_cases.rs", "benches/budgets.toml",
    "crates/basecraft-ops/benches/interaction/fixtures.rs",
    "crates/basecraft-ops/benches/interaction/memory_cases.rs",
    "crates/basecraft-io/src/heap.rs", "crates/basecraft-io/Cargo.toml",
    "crates/basecraft-ops/Cargo.toml")


def runner_identity():
    return hashlib.sha256(b"".join((ROOT / name).read_bytes() for name in RUNNER_INPUTS)).hexdigest()


def json_object(path, label):
    return parse_object(external(path).read_bytes(), label)


def parse_object(content, label):
    value = json.loads(content)
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be a JSON object")
    return value


def require_hash(value, label, length=64):
    if not isinstance(value, str) or not re.fullmatch(r"[a-f0-9]{%d}" % length, value):
        raise ValueError(f"{label} requires a lowercase {length}-digit identity")
    return value


def provision_baseline(args, runner_hash, descriptor, budgets):
    """Validate externally pinned inputs before any compiler/benchmark process.

    The trusted receipt checksum comes from site configuration, not the receipt.
    Claims about the actual runtime remain reviewed caller attestations.
    """
    require_hash(args.provision_sha256, "provision checksum")
    receipt_bytes = external(args.provision).read_bytes()
    if hashlib.sha256(receipt_bytes).hexdigest() != args.provision_sha256:
        raise ValueError("provision receipt checksum mismatch")
    receipt = parse_object(receipt_bytes, "provision receipt")
    if type(receipt.get("schema_version")) is not int or receipt["schema_version"] != 1:
        raise ValueError("unsupported provision receipt schema_version")
    for key in ("reviewed_by", "review_reference"):
        if not isinstance(receipt.get(key), str) or not receipt[key].strip():
            raise ValueError(f"provision receipt requires {key}")
    try:
        expires = datetime.fromisoformat(receipt["valid_until_utc"])
        if expires.utcoffset() is None or expires <= datetime.now(timezone.utc):
            raise ValueError("expired or timezone-less provision receipt")
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError(f"invalid provision valid_until_utc: {error}") from error
    baseline_bytes = external(args.baseline).read_bytes()
    runtime_bytes = external(args.runtime_receipt).read_bytes()
    for key, actual in (("baseline_sha256", hashlib.sha256(baseline_bytes).hexdigest()),
                        ("environment_sha256", descriptor_identity(descriptor)),
                        ("runtime_sha256", hashlib.sha256(runtime_bytes).hexdigest()),
                        ("runner_sha256", runner_hash)):
        require_hash(receipt.get(key), key)
        if receipt[key] != actual:
            raise ValueError(f"provision {key} mismatch; review new inputs, do not relabel samples")
    runtime = parse_object(runtime_bytes, "runtime receipt")
    for key in ("target", "flags", "storage", "power_policy", "load_policy"):
        if not isinstance(runtime.get(key), str) or not runtime[key].strip():
            raise ValueError(f"runtime receipt requires explicit {key}")
    for key in ("image_sha256", "toolchain_sha256"):
        require_hash(runtime.get(key), f"runtime {key}")
    if descriptor.get("reviewed_runtime") != runtime:
        raise ValueError("runtime receipt differs from descriptor reviewed_runtime")
    baseline = parse_object(baseline_bytes, "baseline report")
    for key, length in (("source_commit", 40), ("executable_sha256", 64)):
        require_hash(baseline.get(key), f"baseline {key}", length)
        if receipt.get("baseline_" + key) != baseline[key]:
            raise ValueError(f"provision baseline_{key} mismatch")
    expected = dict(schema_version=SCHEMA_VERSION, machine=args.machine, profile="release",
                    protocol=PROTOCOL, runner_sha256=runner_hash,
                    environment_sha256=descriptor_identity(descriptor))
    for key, value in expected.items():
        if type(baseline.get(key)) is not type(value) or baseline[key] != value:
            raise ValueError(f"baseline {key} incompatible with invocation")
    scenarios = baseline.get("scenarios")
    if not isinstance(scenarios, dict):
        raise ValueError("baseline scenarios must be an object")
    for name in COMPACT_SCENARIOS:
        sample = scenarios.get(name)
        if not isinstance(sample, dict):
            raise ValueError(f"baseline requires {name} object")
        work = dict(scenario=name, operations=1000 if name in (
            "edit-alignment", "undo-alignment") else 1, fixture_bytes=131360,
            rows=32, columns=4096, edits=0 if name == "open-alignment" else 1000,
            source_commit=baseline["source_commit"], debug_assertions=False,
            status="measured", unit="ns/operation")
        for key, value in work.items():
            if type(sample.get(key)) is not type(value) or sample[key] != value:
                raise ValueError(f"baseline {name}: incompatible {key}")
        if type(sample.get("samples")) is not int or sample["samples"] < budgets["minimum_gate_samples"]:
            raise ValueError(f"baseline {name}: insufficient samples")
        for key in ("median", "minimum", "maximum", "mad"):
            value = sample.get(key)
            try:
                finite = type(value) in (int, float) and math.isfinite(value)
            except OverflowError:
                finite = False
            if not finite or value < 0 or (key != "mad" and value == 0):
                raise ValueError(f"baseline {name}: invalid {key}")
        if not sample["minimum"] <= sample["median"] <= sample["maximum"]:
            raise ValueError(f"baseline {name}: inconsistent timing range")
    return baseline, receipt["runtime_sha256"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--machine", required=True, help="user-chosen neutral label, never an actual hostname")
    parser.add_argument("--cache", required=True, type=Path, help="explicit external output and fixture cache")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--gate", choices=("gross", "benchmark"),
                        help="opt-in strict 2x or 1.3x timing gate; requires all compact scenarios")
    parser.add_argument("--environment", type=Path, help="external reviewed environment JSON descriptor")
    parser.add_argument("--provision", type=Path, help="external reviewed baseline provisioning receipt")
    parser.add_argument("--provision-sha256", help="receipt checksum pinned by external site configuration")
    parser.add_argument("--runtime-receipt", type=Path, help="current reviewed runtime attestation JSON")
    parser.add_argument("--compact", action="store_true", help="only compact Session workloads")
    parser.add_argument("--baseline", type=Path, help="reviewed report.json from a compatible run")
    parser.add_argument("--history", type=Path, help="opt-in external JSONL summary history (review before publishing)")
    parser.add_argument("--scenario", action="append", choices=SCENARIOS + FRAME_SCENARIOS + TREE_SCENARIOS + HEAP_SCENARIOS,
                        help="measure selected Session or frame scenarios (repeatable)")
    parser.add_argument("--large", action="store_true", help="also open 1 GB decimal FASTA (may refuse)")
    parser.add_argument("--executable", type=Path, help="reuse a release bench binary built with this source revision")
    parser.add_argument("--revision", help="exact archive revision when .git is absent")
    args = parser.parse_args()
    if args.samples < 3 or not re.fullmatch(r"[A-Za-z0-9_-]{1,40}", args.machine):
        parser.error("need at least 3 samples and a neutral alphanumeric machine label")
    if args.compact and (args.scenario or args.large):
        parser.error("--compact cannot be combined with --scenario or --large")
    selected = COMPACT_SCENARIOS if args.compact else (args.scenario or SCENARIOS)
    if args.gate and (not args.baseline or not args.environment):
        parser.error("strict gate requires --baseline and --environment")
    if args.gate and not set(COMPACT_SCENARIOS).issubset(selected):
        parser.error("strict gate requires all compact Session scenarios")
    provisioning = any((args.provision, args.provision_sha256, args.runtime_receipt))
    if provisioning and not all((args.provision, args.provision_sha256, args.runtime_receipt,
                                 args.baseline, args.environment, args.gate, args.compact)):
        parser.error("provisioning requires --provision, --provision-sha256, --runtime-receipt, "
                     "--baseline, --environment, --gate and --compact")
    if provisioning and args.samples < 5:
        parser.error("provisioned timing gate requires at least 5 samples")
    heap_mode = any(n.startswith("heap-") for n in (args.scenario or ()))
    if heap_mode and (args.large or any(not n.startswith("heap-") for n in args.scenario)):
        parser.error("run heap instrumentation separately from wall timings")
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
    runner_hash = runner_identity()
    # Validate before creating a run or spawning Cargo/the benchmark.
    descriptor = json_object(args.environment, "environment descriptor") if args.environment else None
    environment_hash = descriptor_identity(descriptor) if descriptor is not None else None
    if provisioning:
        baseline, runtime_hash = provision_baseline(args, runner_hash, descriptor, budgets["comparison"])
    else:
        baseline = json.loads(args.baseline.read_text()) if args.baseline else None
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
                                "-p", "basecraft-ops", "--bench", "interaction", "--message-format=json",
                                *(["--features", "test-heap"] if heap_mode else [])],
                               cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, check=True)
        artifacts = [json.loads(line) for line in build.stdout.splitlines()]
        executable = Path(next(a["executable"] for a in artifacts if a.get("executable")
                               and a.get("target", {}).get("name") == "interaction"))
    report = {"schema_version": SCHEMA_VERSION, "source_commit": revision, "profile": "release",
              "machine": args.machine, "protocol": PROTOCOL, "runner_sha256": runner_hash,
              "executable_sha256": digest(executable), "date_utc": datetime.now(timezone.utc).isoformat(),
              "cache": "Session: warm OS/index cache; frame: cold view for first, warmed view for warm/scroll; see scenario work",
              "scenarios": {}}
    if args.environment:
        report["environment_sha256"] = environment_hash
    if provisioning:
        report["provision_sha256"] = args.provision_sha256
        report["runtime_sha256"] = runtime_hash
    failed = False
    for name in (*selected, *(("open-fasta-1gb",) if args.large else ())):
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
            summary = report["scenarios"][name]
            value = f"{summary['median']:.0f} requested bytes" if heap_mode else f"{summary['median'] / 1e6:.3f} ms/operation"
            print(f"{name}: {value} ({len(samples)} samples)", flush=True)
    report["comparison"] = compare(report, baseline, budgets["comparison"])
    if args.gate:
        report["gate"] = timing_gate(report, baseline, budgets["comparison"], args.gate)
        failed |= report["gate"]["status"] != "passed"
    report_path = run_dir / "report.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    if history:
        history.parent.mkdir(parents=True, exist_ok=True)
        with history.open("a") as stream:
            stream.write(json.dumps(report) + "\n")
    print(f"comparison: {report['comparison']['status']}")
    if args.gate:
        print(f"timing gate: {report['gate']['status']}")
        for error in report["gate"]["errors"]:
            print(error, file=sys.stderr)
    else:
        print("timing gate: off (opt in with --gate)")
    print(f"report: {report_path}")
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        sys.exit(f"benchmark failed: {error}")
