"""Adapter checks that do not require compiling or launching r8s."""

import unittest
from pathlib import Path
import tempfile

from r8s import approximate_objective, engine_identity, parse_cv, parse_fit, script, settings
from tree import neutral_newick, parse

TREE = "((A:2,B:2)Clade:1,C:3)Root;"
BASE = {"branch_units": "substitution_counts", "sites": 10, "root_age": 1,
        "time_unit": "relative units", "penalty": "add", "lambda": 10,
        "starts": 3, "seed": 9, "constraints": []}


class AdapterTests(unittest.TestCase):
    def test_tree_preserves_labels_and_lengths_in_source(self):
        root = parse("(('A B':2,'O''Brien':3)Clade:1,C:4)Root;")
        self.assertEqual(root.label, "Root")
        self.assertEqual(root.children[0].children[0].label, "A B")
        self.assertIn("T000001:2", neutral_newick(root))
        self.assertEqual(root.children[0].length_text, "1")

    def test_rejects_invalid_tree_without_repair(self):
        for value in ("(A:1,B:0)R;", "(A:1,B:-1)R;", "(A:1,B:NaN)R;",
                      "((A:1,B:1):1,C)R;", "(A:1,B:1,C:1,D:1)R;",
                      "((A:1,A:1):1,C:1)R;", "((A:1,B:1):1,C:1); extra"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                parse(value)

    def test_forced_r8s_count_rounding_is_rejected(self):
        root = parse("((A:0.001,B:0.002):0.001,C:0.003);")
        params = dict(BASE, branch_units="substitutions_per_site", sites=1000)
        self.assertEqual(settings(params, root)["sites"], 1000)
        with self.assertRaisesRegex(ValueError, "round"):
            settings(dict(params, sites=100), root)
        with self.assertRaisesRegex(ValueError, "round"):
            settings(BASE, parse("((A:0.1,B:2):1,C:3);"))

    def test_constraints_and_commands(self):
        root = parse(TREE)
        params = dict(BASE, constraints=[{"node": "Clade", "min_age": 0.5, "max_age": 0.5}])
        options = settings(params, root)
        self.assertEqual(options["constraints"][0]["node"], "N000002")
        self.assertIn("fixage taxon=N000002 age=0.5;", script(root, options, lam=10))
        self.assertIn("round=no", script(root, options, lam=10))
        with self.assertRaisesRegex(ValueError, "Infeasible"):
            settings(dict(BASE, constraints=[{"node": "Clade", "min_age": 1, "max_age": 1}]), root)

    def test_cv_selection_rejects_failed_grid(self):
        grid = {"log10_start": 0, "log10_step": 1, "count": 2}
        prefix = "Results of cross validation analysis for tree input\n"
        rows, chosen = parse_cv(prefix + "0 1 4 3 (Good)\n1 10 2 1 (Good)\nOptimum: 1.00 10 2 1", grid)
        self.assertEqual(chosen, 10)
        self.assertEqual([row["chi_square_error"] for row in rows], [3, 1])
        with self.assertRaisesRegex(ValueError, "did not complete"):
            parse_cv(prefix + "0 1 4 3 (Good)\n1 10 2 1 (Failed)\nOptimum: 0.00 1 4 3", grid)
        with self.assertRaisesRegex(ValueError, "did not complete"):
            parse_cv(prefix + "0 1 4 3 (Good)\nOptimum: 0.00 1 4 3", grid)
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            parse_cv(prefix + "0 1 4 3 (Good)\n1 10 2 1 (Good)\nOptimum: 0.50 1 4 3", grid)

    def test_output_requires_convergence_and_full_topology(self):
        root = parse(TREE)
        header = "Using optimization from starting point 1 as best estimate\n PASSED FAILED(1)\nResult codes:\n"
        ages = "T000001 * [0] -- -- 0.00 1.0000e+00 1.0000e+00\n" \
               "T000002 * [0] -- -- 0.00 1.0000e+00 1.0000e+00\n" \
               "T000003 * [0] -- -- 0.00 1.0000e+00 1.0000e+00\n" \
               "N000002 * [0] -- -- 0.50 1.0000e+00 1.0000e+00\n"
        output = header + ages + "tree input = ((T000001:0.500000,T000002:0.500000)N000002:0.500000,T000003:1.000000)N000001;\n"
        _, nodes, _ = parse_fit(output, root, 1, [])
        self.assertEqual(len(nodes), 5)
        objective = approximate_objective(root, nodes, "substitution_counts", 10, "add", 10)
        self.assertAlmostEqual(objective["roughness"], 0)
        self.assertTrue(objective["penalized_objective"] > 0)
        with self.assertRaisesRegex(ValueError, "topology"):
            parse_fit(output.replace("T000002:0.500000", "T000003:0.500000").replace("T000003:1.000000", "T000002:1.000000"), root, 1, [])
        with self.assertRaisesRegex(ValueError, "nonconverged"):
            parse_fit(output.replace("PASSED FAILED(1)", "FAILED(1) PASSED"), root, 1, [])
        with self.assertRaisesRegex(ValueError, "exactly one"):
            parse_fit(header + ages, root, 1, [])
        with self.assertRaisesRegex(ValueError, "fitted branch rates"):
            parse_fit(output.replace("T000003 * [0] -- -- 0.00 1.0000e+00 1.0000e+00\n", ""), root, 1, [])

    def test_missing_pinned_engine_fails_before_analysis(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "not packaged"):
                engine_identity(Path(directory))


if __name__ == "__main__":
    unittest.main()
