"""Reporting regressions; synthetic records test math/compatibility, never timings."""
import copy
import unittest

from run import compare, summarize


class ReportTests(unittest.TestCase):
    def sample(self, duration):
        return dict(scenario="edit-alignment", operations=1000, fixture_bytes=131360,
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
        return dict(schema_version=1, machine="neutral", profile="release",
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
