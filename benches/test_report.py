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


if __name__ == "__main__":
    unittest.main()
