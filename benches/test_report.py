"""Reporting regressions; synthetic records test math/compatibility, never timings."""
import copy
import unittest
from unittest.mock import patch
from pathlib import Path
import tempfile
import subprocess
import json
import contextlib
import io

import run

from run import compare, summarize


class ReportTests(unittest.TestCase):
    def sample(self, duration):
        return dict(schema_version=2, output_bytes=131360, scenario="edit-alignment", operations=1000, fixture_bytes=131360,
                    rows=32, columns=4096, edits=1000, source_commit="a" * 40,
                    debug_assertions=False, verified=True, duration_ns=duration)

    def test_median_normalizes_per_operation(self):
        result = summarize([self.sample(n) for n in (3000, 9000, 1000, 5000)])
        self.assertEqual(result["median"], 4)
        self.assertEqual(result["samples"], 4)
        self.assertEqual(result["unit"], "ns/operation")

    def test_refusal_or_inconsistent_samples_are_not_measurements(self):
        for bad in ({"verified": False}, {"operations": 1}, {"duration_ns": 0}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                summarize([self.sample(1000), {**self.sample(2000), **bad}])
        with self.assertRaises(ValueError):
            summarize([])

    def report(self):
        return dict(schema_version=2, machine="neutral", profile="release",
                    protocol="session-warm-v1", runner_sha256="runner",
                    scenarios={"edit-alignment": summarize([self.sample(1000)] * 3)})

    def test_missing_or_wrong_baseline_cannot_pass(self):
        current = self.report()
        budgets = dict(benchmark_ratio=1.3, gross_ci_ratio=2)
        self.assertEqual(compare(current, None, budgets)["status"], "unavailable")
        for key in ("machine", "profile", "protocol", "runner_sha256", "schema_version"):
            previous = {**current, key: "different"}
            self.assertEqual(compare(current, previous, budgets)["status"], "unavailable")
        previous = copy.deepcopy(current)
        previous["scenarios"]["edit-alignment"]["fixture_bytes"] += 1
        result = compare(current, previous, budgets)
        self.assertEqual(result["scenarios"]["edit-alignment"]["status"], "unavailable")

    def test_save_output_size_does_not_change_input_compatibility(self):
        samples = [{**self.sample(1000), "scenario": "save-alignment", "operations": 1,
                    "output_bytes": size} for size in (133536, 132000, 140000)]
        current = self.report()
        current["scenarios"] = {"save-alignment": summarize(samples)}
        previous = copy.deepcopy(current)
        previous["scenarios"]["save-alignment"]["output_bytes"] = 150000
        current["scenarios"]["save-alignment"]["output_bytes"] = 133536
        budgets = dict(benchmark_ratio=1.3, gross_ci_ratio=2)
        result = compare(current, previous, budgets)
        self.assertEqual(result["scenarios"]["save-alignment"]["status"], "compared")
        self.assertEqual(result["scenarios"]["save-alignment"]["ratio"], 1)
        previous["scenarios"]["save-alignment"]["fixture_bytes"] += 1
        result = compare(current, previous, budgets)
        self.assertEqual(result["scenarios"]["save-alignment"]["status"], "unavailable")

    def test_frame_work_identity_and_output_counts(self):
        sample = {**self.sample(1000), "scenario": "frame-scroll-alignment",
                  "operations": 200, "work": {"width": 3840, "frames": 200},
                  "output_counts": [5000] * 200}
        current = self.report()
        current["scenarios"] = {sample["scenario"]: summarize([sample] * 3)}
        previous = copy.deepcopy(current)
        budgets = dict(benchmark_ratio=1.3, gross_ci_ratio=2)
        self.assertEqual(compare(current, previous, budgets)["scenarios"][sample["scenario"]]["status"], "compared")
        previous["scenarios"][sample["scenario"]]["work"]["width"] = 800
        self.assertEqual(compare(current, previous, budgets)["scenarios"][sample["scenario"]]["status"], "unavailable")
        for bad in ({"work": {}}, {"output_counts": []}, {"output_counts": [0] * 200}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                summarize([sample, {**sample, **bad}])
        current["scenarios"][sample["scenario"]] = {"status": "failed_or_refused", "completed_samples": 2}
        self.assertEqual(compare(current, previous, budgets)["scenarios"][sample["scenario"]]["status"], "unavailable")

    def test_tree_work_identity(self):
        sample = {**self.sample(1000), "scenario": "tree-midpoint-50000",
                  "operations": 1, "rows": 50000, "columns": None, "edits": 1,
                  "work": {"tips": 50000, "shape": "balanced", "branches": "varying",
                           "method": "midpoint", "recovery": "durable", "operation_count": 1}}
        current = self.report()
        current["scenarios"] = {sample["scenario"]: summarize([sample] * 3)}
        budgets = dict(benchmark_ratio=1.3, gross_ci_ratio=2)
        self.assertEqual(compare(current, copy.deepcopy(current), budgets)["scenarios"][sample["scenario"]]["status"], "compared")
        for key in sample["work"]:
            previous = copy.deepcopy(current)
            previous["scenarios"][sample["scenario"]]["work"][key] = "changed"
            self.assertEqual(compare(current, previous, budgets)["scenarios"][sample["scenario"]]["status"], "unavailable")
            changed = copy.deepcopy(sample)
            changed["work"][key] = "changed"
            with self.assertRaises(ValueError):
                summarize([sample, changed])
        missing = copy.deepcopy(sample)
        del missing["work"]
        with self.assertRaises(KeyError):
            summarize([missing])

    def test_failed_tree_child_is_retained_and_next_scenario_runs(self):
        self.failed_child("tree-midpoint-50000", "tree-open-50000")

    def test_failed_frame_child_is_retained_and_next_scenario_runs(self):
        self.failed_child("frame-first-alignment", "frame-warm-alignment")

    def failed_child(self, failed, following):
        sample = {**self.sample(1000), "scenario": following, "operations": 1,
                  "work": {"frames": 1}, "output_counts": [5000]}
        if following.startswith("heap-"):
            sample = {**self.heap_sample(), "scenario": following}
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            executable = cache / "binary"
            executable.write_bytes(b"mock release binary")
            argv = ["run.py", "--machine", "neutral", "--cache", directory,
                    "--samples", "3", "--revision", "a" * 40, "--executable", str(executable),
                    "--scenario", failed, "--scenario", following]
            children = [subprocess.CompletedProcess([], 1, "", "geometry failed")] + [
                subprocess.CompletedProcess([], 0, json.dumps(sample), "") for _ in range(3)]
            with patch("sys.argv", argv), patch.object(run.subprocess, "check_output", side_effect=[b"", "a" * 40]), patch.object(run.subprocess, "run", side_effect=children), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(run.main(), 1)
            report_path = next(cache.glob("interaction-*/report.json"))
            report = json.loads(report_path.read_text())
            self.assertEqual(report["scenarios"][failed]["status"], "failed_or_refused")
            self.assertEqual(report["scenarios"][failed]["completed_samples"], 0)
            self.assertEqual(report["scenarios"][following]["samples"], 3)
            self.assertEqual((report_path.parent / f"{failed}-0/stderr.txt").read_text(), "geometry failed")

    def heap_sample(self):
        return {**self.sample(1000), "scenario": "heap-open-100mb", "operations": 1,
                "work": {"instrumentation": "system-requested-live-v1", "unit": "requested bytes"},
                "heap": {"baseline_bytes": 10, "peak_bytes": 210, "incremental_peak_bytes": 200,
                         "live_after_open_bytes": 100, "live_after_drop_bytes": 10,
                         "successful_reallocations": 3, "failed_reallocations": 0, "failed_allocations": 0}}

    def test_heap_units_math_and_identity(self):
        sample = self.heap_sample()
        result = summarize([sample] * 3)
        self.assertEqual(result["median"], 200)
        self.assertEqual(result["unit"], "requested bytes")
        self.assertEqual(result["heap_medians"]["peak_bytes"], 210)
        for key, value in (("peak_bytes", 1), ("incremental_peak_bytes", 199),
                           ("failed_reallocations", 1), ("failed_allocations", 1),
                           ("live_after_drop_bytes", -1), ("baseline_bytes", True)):
            changed = copy.deepcopy(sample)
            changed["heap"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                summarize([changed])
        current = self.report()
        current["scenarios"] = {sample["scenario"]: result}
        previous = copy.deepcopy(current)
        previous["scenarios"][sample["scenario"]]["work"]["instrumentation"] = "other"
        self.assertEqual(compare(current, previous, dict(benchmark_ratio=1.3, gross_ci_ratio=2))[
            "scenarios"][sample["scenario"]]["status"], "unavailable")
        changed = copy.deepcopy(sample)
        changed["work"]["instrumentation"] = "other"
        with self.assertRaises(ValueError):
            summarize([sample, changed])

    def test_failed_heap_child_is_retained(self):
        self.failed_child("heap-open-100mb", "heap-open-compact")

    def test_old_size_semantics_require_new_samples(self):
        old_sample = self.sample(1000)
        del old_sample["schema_version"]
        with self.assertRaisesRegex(ValueError, "unsupported sample schema"):
            summarize([old_sample])
        previous = self.report()
        previous["schema_version"] = 1
        result = compare(self.report(), previous, dict(benchmark_ratio=1.3, gross_ci_ratio=2))
        self.assertEqual(result["status"], "unavailable")
        self.assertEqual(result["reason"], "baseline schema_version differs")

    def gate_report(self):
        report = self.report()
        report.update(environment_sha256="e" * 64, source_commit="a" * 40,
                      executable_sha256="b" * 64)
        report["scenarios"] = {name: summarize([
            {**self.sample(1000), "scenario": name}] * 5) for name in run.COMPACT_SCENARIOS}
        return report

    def gate(self, current, previous, mode="gross"):
        return run.timing_gate(current, previous, dict(
            benchmark_ratio=1.3, gross_ci_ratio=2, minimum_gate_samples=5), mode)

    def test_gate_detects_injected_slowdown_and_threshold_boundary(self):
        previous = self.gate_report()
        for factor, gross, benchmark in ((1, "passed", "passed"), (1.3, "passed", "passed"),
                                          (1.31, "passed", "failed"), (2, "passed", "failed"),
                                          (2.01, "failed", "failed")):
            current = copy.deepcopy(previous)
            current["scenarios"]["edit-alignment"]["median"] *= factor
            self.assertEqual(self.gate(current, previous)["status"], gross)
            self.assertEqual(self.gate(current, previous, "benchmark")["status"], benchmark)
        current["source_commit"] = "c" * 40
        current["executable_sha256"] = "d" * 64
        current["scenarios"]["edit-alignment"]["median"] = 1
        self.assertEqual(self.gate(current, previous)["status"], "passed")

    def test_gate_fails_closed_for_missing_and_incompatible_metadata(self):
        current = self.gate_report()
        self.assertEqual(self.gate(current, None)["status"], "failed")
        for key in ("machine", "profile", "protocol", "runner_sha256", "schema_version", "environment_sha256"):
            previous = copy.deepcopy(current)
            previous[key] = "different"
            with self.subTest(key=key):
                self.assertEqual(self.gate(current, previous)["status"], "failed")
            del previous[key]
            self.assertEqual(self.gate(current, previous)["status"], "failed")
        for name in run.COMPACT_SCENARIOS:
            for side in ("current", "baseline"):
                for bad in (None, {"status": "failed_or_refused"}, {"samples": 3},
                            {"fixture_bytes": 99}, {"median": float("nan")},
                            {"median": float("inf")}, {"median": 0}, {"median": True}):
                    pair = {"current": copy.deepcopy(current), "baseline": copy.deepcopy(current)}
                    if bad is None:
                        del pair[side]["scenarios"][name]
                    else:
                        pair[side]["scenarios"][name].update(bad)
                    with self.subTest(name=name, side=side, bad=bad):
                        self.assertEqual(self.gate(pair["current"], pair["baseline"])["status"], "failed")

    def test_runner_gate_exit_and_retained_report(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            executable = cache / "binary"
            executable.write_bytes(b"mock release binary")
            environment = cache / "environment.json"
            environment.write_text(json.dumps({key: "reviewed" for key in (
                "hardware", "toolchain", "runtime", "storage", "build", "isolation")}))
            baseline = None
            for factor, expected, changed_environment in ((1, 0, False), (1, 0, False),
                                                         (2.01, 1, False), (1, 1, True)):
                argv = ["run.py", "--machine", "neutral", "--cache", directory,
                        "--samples", "5", "--revision", "a" * 40, "--executable", str(executable),
                        "--compact", "--environment", str(environment)]
                if baseline:
                    argv += ["--gate", "gross", "--baseline", str(baseline)]
                if changed_environment:
                    descriptor = json.loads(environment.read_text())
                    descriptor["storage"] = "changed"
                    environment.write_text(json.dumps(descriptor))
                children = [subprocess.CompletedProcess([], 0, json.dumps({
                    **self.sample(int(1000 * factor)), "scenario": name}), "")
                    for name in run.COMPACT_SCENARIOS for _ in range(5)]
                before = set(cache.glob("interaction-*/report.json"))
                with patch("sys.argv", argv), patch.object(run.subprocess, "check_output", side_effect=[b"", "a" * 40]), patch.object(run.subprocess, "run", side_effect=children), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(run.main(), expected)
                path, = set(cache.glob("interaction-*/report.json")) - before
                report = json.loads(path.read_text())
                if baseline:
                    self.assertEqual(report["gate"]["status"], "passed" if expected == 0 else "failed")
                else:
                    baseline = path

    def test_environment_descriptor_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "environment.json"
            descriptor = {key: "reviewed value" for key in (
                "hardware", "toolchain", "runtime", "storage", "build", "isolation")}
            path.write_text(json.dumps(descriptor))
            identity = run.environment_identity(path)
            path.write_text(json.dumps(descriptor, sort_keys=True, indent=2))
            self.assertEqual(run.environment_identity(path), identity)
            descriptor["toolchain"] = "changed"
            path.write_text(json.dumps(descriptor))
            self.assertNotEqual(run.environment_identity(path), identity)
            del descriptor["storage"]
            path.write_text(json.dumps(descriptor))
            with self.assertRaises(ValueError):
                run.environment_identity(path)

    def test_variability_and_invalid_timing_samples(self):
        result = summarize([self.sample(n) for n in (1000, 3000, 9000)])
        self.assertEqual((result["minimum"], result["maximum"], result["mad"]), (1, 9, 2))
        for bad in (float("nan"), float("inf"), True, -1):
            with self.assertRaises(ValueError):
                summarize([{**self.sample(1000), "duration_ns": bad}])

    def test_ratios_are_observations_and_refusals_are_explicit(self):
        previous = self.report()
        current = copy.deepcopy(previous)
        current["scenarios"]["edit-alignment"]["median"] = 2.1
        result = compare(current, previous, dict(benchmark_ratio=1.3, gross_ci_ratio=2))
        self.assertFalse(result["gate_enabled"])
        comparison = result["scenarios"]["edit-alignment"]
        self.assertTrue(comparison["over_30_percent"])
        self.assertTrue(comparison["over_2x"])
        current["scenarios"]["edit-alignment"] = {"status": "failed_or_refused"}
        result = compare(current, previous, dict(benchmark_ratio=1.3, gross_ci_ratio=2))
        self.assertEqual(result["scenarios"]["edit-alignment"]["status"], "unavailable")


class ProvisionTests(unittest.TestCase):
    sample = ReportTests.sample
    report = ReportTests.report
    gate_report = ReportTests.gate_report
    """Real CLI subprocesses with explicitly synthetic children; no Cargo/timing."""

    def setUp(self):
        import shutil
        import sys
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.source = self.base / "source"
        for name in run.RUNNER_INPUTS:
            target = self.source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(run.ROOT / name, target)
        self.cache = self.base / "cache"
        self.baseline = self.base / "baseline.json"
        self.environment = self.base / "environment.json"
        self.runtime = self.base / "runtime.json"
        self.receipt = self.base / "provision.json"
        self.marker = self.base / "spawned"
        child = self.base / "synthetic-child"
        child.write_text("#!" + sys.executable + "\n" +
            "import json, os, sys\nfrom pathlib import Path\n" +
            f"Path({str(self.marker)!r}).touch()\n" +
            "name = sys.argv[1]\n" +
            "if os.environ.get('SYNTHETIC_REFUSE') and name == 'edit-alignment':\n" +
            "    sys.exit('synthetic child refusal')\n" +
            "print(json.dumps(dict(schema_version=2, verified=True, scenario=name, " +
            "duration_ns=1000, operations=1000 if name in ('edit-alignment', 'undo-alignment') else 1, " +
            "fixture_bytes=131360, rows=32, columns=4096, edits=0 if name == 'open-alignment' else 1000, " +
            "source_commit='a'*40, debug_assertions=False)))\n")
        child.chmod(0o755)
        self.child = child
        # A regression must never launch a real compiler on the test machine.
        cargo = self.base / "cargo"
        cargo.write_text("#!" + sys.executable + "\nfrom pathlib import Path\n" +
                         f"Path({str(self.marker)!r}).touch()\nraise SystemExit(77)\n")
        cargo.chmod(0o755)
        runtime = dict(target="synthetic-target", flags="default features; no extra flags",
                       image_sha256="1" * 64, toolchain_sha256="2" * 64,
                       storage="synthetic storage identity", power_policy="synthetic fixed policy",
                       load_policy="synthetic exclusive lock and load policy")
        self.write(self.runtime, runtime)
        descriptor = {key: "synthetic" for key in (
            "hardware", "toolchain", "runtime", "storage", "build", "isolation")}
        descriptor["reviewed_runtime"] = runtime
        self.write(self.environment, descriptor)
        report = self.gate_report()
        report["runner_sha256"] = run.runner_identity()
        report["environment_sha256"] = run.environment_identity(self.environment)
        for name, sample in report["scenarios"].items():
            sample.update(operations=1000 if name in ("edit-alignment", "undo-alignment") else 1,
                          edits=0 if name == "open-alignment" else 1000)
            sample.update(median=1000 / sample["operations"], minimum=1000 / sample["operations"],
                          maximum=1000 / sample["operations"], mad=0)
        self.write(self.baseline, report)
        self.pin()
        self.argv = [sys.executable, str(self.source / "benches/run.py"),
                     "--machine", "neutral", "--cache", str(self.cache), "--revision", "a" * 40,
                     "--compact", "--samples", "5", "--gate", "gross", "--baseline", str(self.baseline),
                     "--environment", str(self.environment), "--runtime-receipt", str(self.runtime),
                     "--provision", str(self.receipt)]

    def write(self, path, value):
        path.write_text(json.dumps(value))

    def pin(self):
        report = json.loads(self.baseline.read_text())
        self.write(self.receipt, dict(schema_version=1, reviewed_by="synthetic reviewer",
            review_reference="synthetic review only", valid_until_utc="2999-01-01T00:00:00+00:00",
            baseline_sha256=run.digest(self.baseline), environment_sha256=run.environment_identity(self.environment),
            runtime_sha256=run.digest(self.runtime), runner_sha256=run.runner_identity(),
            baseline_source_commit=report["source_commit"], baseline_executable_sha256=report["executable_sha256"]))

    def invoke(self, executable=True, checksum=None, refuse=False):
        import os
        env = os.environ.copy()
        env["PATH"] = str(self.base) + os.pathsep + env.get("PATH", "")
        if refuse:
            env["SYNTHETIC_REFUSE"] = "1"
        return subprocess.run(self.argv + ["--provision-sha256", checksum or (run.digest(self.receipt) if self.receipt.exists() else "0" * 64)] +
            (["--executable", str(self.child)] if executable else []), env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_provisioned_cli_pass_and_child_failure_diagnostics(self):
        for refuse, code in ((False, 0), (True, 1)):
            result = self.invoke(refuse=refuse)
            self.assertEqual(result.returncode, code, result.stderr)
            path = Path(result.stdout.split("report: ")[1].strip())
            report = json.loads(path.read_text())
            self.assertEqual(report["gate"]["status"], "failed" if refuse else "passed")
            self.assertEqual(report["scenarios"]["save-alignment"]["samples"], 5)
            self.assertEqual(report["provision_sha256"], run.digest(self.receipt))
            if refuse:
                self.assertEqual(report["scenarios"]["edit-alignment"]["exit_code"], 1)
                self.assertIn("synthetic child refusal", (path.parent / "edit-alignment-0/stderr.txt").read_text())

    def assert_preflight_failure(self, diagnostic):
        # Without --executable this would compile; with it this would run the child.
        for executable in (False, True):
            result = self.invoke(executable=executable)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(diagnostic, result.stderr)
            self.assertNotIn("Traceback", result.stderr)
            self.assertFalse(self.marker.exists())
            self.assertEqual(list(self.cache.glob("interaction-*")), [])

    def test_invalid_provisioning_never_spawns_compiler_or_benchmark(self):
        original = {p: p.read_bytes() for p in (self.baseline, self.environment, self.runtime, self.receipt)}
        cases = [
            (self.receipt, [], "JSON object", False),
            (self.receipt, {"schema_version": 99}, "schema_version", False),
            (self.baseline, [], "JSON object", True),
            (self.baseline, {"scenarios": []}, "scenarios", True),
        ]
        for path, change, diagnostic, repin in cases:
            for p, content in original.items():
                p.write_bytes(content)
            if isinstance(change, dict):
                value = json.loads(path.read_text())
                value.update(change)
            else:
                value = change
            self.write(path, value)
            if repin:
                receipt = json.loads(self.receipt.read_text())
                receipt["baseline_sha256"] = run.digest(self.baseline)
                self.write(self.receipt, receipt)
            with self.subTest(change=change):
                self.assert_preflight_failure(diagnostic)
        for p, content in original.items():
            p.write_bytes(content)
        for key, value, diagnostic in (("valid_until_utc", "2000-01-01T00:00:00Z", "expired"),
                ("baseline_source_commit", "b" * 40, "baseline_source_commit"),
                ("runner_sha256", "0" * 64, "runner_sha256")):
            self.receipt.write_bytes(original[self.receipt])
            receipt = json.loads(self.receipt.read_text())
            receipt[key] = value
            self.write(self.receipt, receipt)
            self.assert_preflight_failure(diagnostic)

    def test_missing_malformed_and_incompatible_files_fail_preflight(self):
        originals = {p: p.read_bytes() for p in (self.baseline, self.environment, self.runtime, self.receipt)}
        for path in originals:
            path.unlink()
            self.assert_preflight_failure("No such file")
            path.write_bytes(originals[path])
            path.write_text("{malformed")
            # Pin malformed baseline/runtime bytes so JSON shape validation is reached.
            if path != self.receipt:
                receipt = json.loads(originals[self.receipt])
                if path != self.environment:
                    receipt["baseline_sha256" if path == self.baseline else "runtime_sha256"] = run.digest(path)
                    self.write(self.receipt, receipt)
            self.assert_preflight_failure("benchmark failed:")
            for p, content in originals.items():
                p.write_bytes(content)
        for key, value in (("schema_version", True), ("machine", "other"),
                           ("runner_sha256", "0" * 64), ("environment_sha256", "0" * 64),
                           ("source_commit", "invalid"), ("scenarios", {})):
            report = json.loads(originals[self.baseline])
            report[key] = value
            self.write(self.baseline, report)
            receipt = json.loads(originals[self.receipt])
            receipt["baseline_sha256"] = run.digest(self.baseline)
            self.write(self.receipt, receipt)
            self.assert_preflight_failure("baseline")
        for p, content in originals.items():
            p.write_bytes(content)
        descriptor = json.loads(self.environment.read_text())
        descriptor["storage"] = "changed storage"
        self.write(self.environment, descriptor)
        self.assert_preflight_failure("environment_sha256")
        # Re-pinning metadata cannot make the old report match a new environment.
        self.pin()
        self.assert_preflight_failure("baseline environment_sha256")
        self.environment.write_bytes(originals[self.environment])
        runtime = json.loads(self.runtime.read_text())
        runtime["power_policy"] = "changed effective policy"
        self.write(self.runtime, runtime)
        self.pin()
        self.assert_preflight_failure("differs from descriptor")

    def test_bad_work_environment_and_checksums_fail_preflight(self):
        original = self.baseline.read_bytes()
        for key, value in (("samples", 4), ("fixture_bytes", 1), ("operations", 1),
                           ("median", float("nan")), ("median", 10 ** 1000), ("debug_assertions", True)):
            report = json.loads(original)
            report["scenarios"]["edit-alignment"][key] = value
            self.write(self.baseline, report)
            self.pin()
            self.assert_preflight_failure("edit-alignment")
        self.baseline.write_bytes(original)
        self.pin()
        result = self.invoke(checksum="0" * 64)
        self.assertIn("checksum mismatch", result.stderr)
        self.runtime.write_text('{}')
        self.assert_preflight_failure("runtime_sha256")
        self.pin()
        self.assert_preflight_failure("runtime receipt requires")



if __name__ == "__main__":
    unittest.main()
