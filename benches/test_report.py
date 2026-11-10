"""Reporting regressions; synthetic records test math/compatibility, never timings."""
import copy
import unittest

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
