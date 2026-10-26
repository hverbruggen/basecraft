"""Noncompiler checks of schema-2 transport and artifact failure behavior."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python" / "src"))

from plugin import _parameters, date_tree  # noqa: E402
from tree import labeled_newick, parse  # noqa: E402


class TransportTests(unittest.TestCase):
    def test_effective_defaults_and_exclusive_smoothing_choice(self):
        settings = _parameters({"lambda": 2}, {}, {})
        self.assertEqual((settings["penalty"], settings["starts"], settings["seed"]),
                         ("log", 3, 1))
        self.assertEqual((settings["max_iterations"], settings["max_seconds"]),
                         (2000, 3600))
        with self.assertRaises(ValueError):
            _parameters({"lambda": 2, "cross_validation": {}}, {}, {})

    def test_native_duration_serializes_without_six_decimal_truncation(self):
        root = parse("((A:1,B:1):1,C:1);")
        durations = {node.key: 0.12345678901234566 for node in root.walk() if node is not root}
        encoded = labeled_newick(root, durations) + ";"
        reopened = parse(encoded)
        self.assertEqual(reopened.children[0].length, 0.12345678901234566)

    def test_native_result_restores_exact_labels_and_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.nwk"
            source.write_text("(('A B':0.011,C:0.012)Clade:0.021,D:0.03)Root;\n")
            chrono, report = root / "chrono.nwk", root / "report.json"
            context = {"inputs": {"tree": [str(source)]},
                       "outputs": {"chronogram": str(chrono), "report": str(report)}}
            parameters = {"branch_units": "substitutions_per_site", "sites": 953,
                          "root_age": 1, "time_unit": "relative units", "penalty": "log",
                          "lambda": 2, "starts": 2, "seed": 7,
                          "constraints": [{"node": "Clade", "min_age": 0.5, "max_age": 0.5}]}
            result = {"ages": [1, 0.5, 0, 0, 0],
                      "rates_per_site": [None, 0.1, 0.2, 0.3, 0.4],
                      "lambda": 2, "objective": 3.5, "optimizer": "argmin test",
                      "observation_convention": "continuous test"}
            with patch("plugin._binary", return_value=(Path("/mock/binary"), "abc")), \
                 patch("plugin._run", return_value=result):
                response = date_tree(None, parameters, context)
            self.assertEqual(len(response["artifacts"]), 2)
            self.assertIn("'A B'", chrono.read_text())
            evidence = json.loads(report.read_text())
            self.assertEqual(evidence["source_newick"], source.read_text())
            self.assertEqual(evidence["effective_settings"]["constraints"][0]["node"], 1)
            self.assertEqual(evidence["nodes"][2]["original_length_text"], "0.011")

    def test_invalid_native_result_publishes_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.nwk"
            source.write_text("((A:0.1,B:0.2):0.3,C:0.4);")
            chrono, report = root / "chrono.nwk", root / "report.json"
            context = {"inputs": {"tree": [str(source)]},
                       "outputs": {"chronogram": str(chrono), "report": str(report)}}
            parameters = {"branch_units": "substitution_counts", "sites": 100,
                          "root_age": 1, "time_unit": "relative units", "penalty": "add", "lambda": 1}
            truncated = {"ages": [1, 0.5], "rates_per_site": [None, 1],
                         "lambda": 1, "objective": 1}
            with patch("plugin._binary", return_value=(Path("/mock/binary"), "abc")), \
                 patch("plugin._run", return_value=truncated), self.assertRaises(ValueError):
                date_tree(None, parameters, context)
            self.assertFalse(chrono.exists())
            self.assertFalse(report.exists())


if __name__ == "__main__":
    unittest.main()
