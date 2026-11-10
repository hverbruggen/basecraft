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
