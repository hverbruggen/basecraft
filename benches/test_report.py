"""Checks for summaries and strict budgets used by the actual runner."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("bench", Path(__file__).with_name("run.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class ReportTests(unittest.TestCase):
    def test_median_is_per_operation(self):
        samples = [dict(scenario="edit", verified=True, duration_ns=n, operations=2)
                   for n in (2_000_000, 4_000_000, 90_000_000)]
        self.assertEqual(bench.summarize(samples), 2)

    def test_unverified_samples_cannot_enter_history(self):
        with self.assertRaises(ValueError):
            bench.summarize([dict(verified=False)])

    def test_missing_budget_is_an_error(self):
        with self.assertRaises(KeyError):
            bench.ceiling("new-scenario", {"limits_ms": {}})
