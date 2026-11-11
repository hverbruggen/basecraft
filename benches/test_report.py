"""Checks for summaries and strict budgets used by the actual runner."""
import importlib.util
from pathlib import Path
import unittest
import json
import tempfile
from types import SimpleNamespace

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

    def test_absolute_gate_and_explicit_history_recording(self):
        with tempfile.TemporaryDirectory() as directory:
            history = Path(directory) / "history.jsonl"
            history.write_text('{"median_ms": 999999, "passed": true}\n')
            before = history.read_bytes()
            args = SimpleNamespace(gate="benchmark", machine="test", history=history, record=False)
            limits = {"limits_ms": {"open-alignment": 100}}
            for value in [101, 1000]:
                self.assertFalse(bench.record("open-alignment", [value], "ms", "test",
                    args, limits, None, "source"))
            self.assertEqual(history.read_bytes(), before)
            args.record = True
            self.assertTrue(bench.record("open-alignment", [50], "ms", "test",
                args, limits, None, "source"))
            self.assertEqual(len(history.read_text().splitlines()), 2)
            self.assertEqual(json.loads(history.read_text().splitlines()[-1])["limit_ms"], 100)
