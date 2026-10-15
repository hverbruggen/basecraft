"""Output grammar/coverage and real-filesystem publication failure regressions."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from output_contract import publish_artifacts, restore_tree


class TreeCoverage(unittest.TestCase):
    def setUp(self):
        self.names = {"t0": "a", "t1": "b", "t2": "c", "t3": "d"}

    def test_complete_leaf_coverage_and_structure(self):
        invalid = [
            "(t0:1,t1:1,t2:1,t3:1,rogue:1);",
            "(t0:1,t1:1,t2:1,t3:1,'rogue name':1);",
            "(t0:1,t1:1,t2:1,t3:1,t4:1);",
            "(t0:1,t1:1,t2:1,t2:1);",
            "(t0:1,t1:1,t2:1);",
            "(t0:1,t1:1,t2:1,t3:1;",
            "t0:1,t1:1,t2:1,t3:1);",
            "(t0:1,,t1:1,t2:1,t3:1);",
            "(t0:1,t1:1,t2:1,t3:1,);",
            "(t0:1 t1:1,t2:1,t3:1);",
            "(t0:1,t1:1,t2:1,t3:1);(rogue:1,x:1);",
            "(t0:1,t1:1,t2:1,t3:1);trailing",
            "(t0:1,t1:1,t2:1,t3:1)",
            "(t0:NaN,t1:1,t2:1,t3:1);",
            "(t0:1e999,t1:1,t2:1,t3:1);",
            "(t0:-1,t1:1,t2:1,t3:1);",
            "(t0:,t1:1,t2:1,t3:1);",
            "(t0:'1',t1:1,t2:1,t3:1);",
            "(t0:1,t1:1,t2:1,'t3:1);",
            "(t0:1,t1:1,t2:1,t3:1);[unfinished",
            "(t0:1,t1:1,t2:1,:1);",
            "(t0:1,t1:1,t2:1,'':1);",
            "",
        ]
        for tree in invalid:
            with self.subTest(tree=tree), self.assertRaisesRegex(RuntimeError, "Invalid IQ-TREE output"):
                restore_tree(tree, self.names)

    def test_restores_only_leaves_preserving_support_lengths_and_comments(self):
        tree = "[&R] (('t0':1e-5, t1[tip]:.2)99:3,(t2:0,t3:1.)t0:4); [tail]"
        expected = "[&R] (('a':1e-5, 'b'[tip]:.2)99:3,('c':0,'d':1.)t0:4); [tail]"
        self.assertEqual(restore_tree(tree, self.names), expected)
        self.assertEqual(restore_tree("(t0,t1,t2,t3);", dict(self.names, t0="a'b")), "('a''b','b','c','d');")

    def test_deep_tree_does_not_use_python_recursion(self):
        depth = 1500
        tree = "(" * depth + "t0" + "".join(f",t{i})" for i in range(1, depth + 1)) + ";"
        names = {f"t{i}": f"s{i}" for i in range(depth + 1)}
        restored = restore_tree(tree, names)
        self.assertEqual(restored.count("'s"), depth + 1)


class Publication(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.paths = [self.root / ("tree.nwk" + suffix) for suffix in ("", ".iqtree", ".log", ".run.json")]
        self.artifacts = [(path, f"new-{i}\n") for i, path in enumerate(self.paths)]
        self.previous = {path: f"previous-{i}\n" for i, path in enumerate(self.paths)}
        for path, content in self.previous.items():
            path.write_text(content)

    def assert_previous(self):
        for path, content in self.previous.items():
            self.assertEqual(path.read_text(), content)
        self.assertEqual(list(self.root.glob(".basecraft-iqtree-publish-*")), [])

    def test_preflight_directory_preserves_prior_tree(self):
        self.paths[-1].unlink()
        self.paths[-1].mkdir()
        with self.assertRaisesRegex(OSError, "previous result preserved"):
            publish_artifacts(self.artifacts)
        for path in self.paths[:-1]:
            self.assertEqual(path.read_text(), self.previous[path])
        self.assertTrue(self.paths[-1].is_dir())

    def test_failure_at_each_replacement_restores_entire_previous_set(self):
        replace = os.replace
        for failure_index in range(len(self.paths)):
            for fail_after_replace in (False, True):
                hit = []

                def fail(source, destination, failure_index=failure_index, hit=hit, fail_after_replace=fail_after_replace):
                    if Path(destination) == self.paths[failure_index] and Path(source).name.startswith("new-"):
                        hit.append(destination)
                        # Earlier files have actually been replaced, not just staged.
                        for path, content in self.artifacts[:failure_index]:
                            self.assertEqual(path.read_text(), content)
                        if fail_after_replace:
                            replace(source, destination)
                        raise OSError("injected replacement error")
                    return replace(source, destination)

                with self.subTest(index=failure_index, after=fail_after_replace), patch("output_contract.os.replace", side_effect=fail), self.assertRaisesRegex(OSError, "previous result preserved"):
                    publish_artifacts(self.artifacts)
                self.assertTrue(hit)
                self.assert_previous()

    def test_failed_new_result_leaves_no_partial_artifacts(self):
        for path in self.paths:
            path.unlink()
        replace = os.replace

        def fail(source, destination):
            if Path(destination) == self.paths[-1]:
                raise OSError("late replacement error")
            return replace(source, destination)

        with patch("output_contract.os.replace", side_effect=fail), self.assertRaisesRegex(OSError, "previous result preserved"):
            publish_artifacts(self.artifacts)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_failed_rollback_retains_prior_artifacts_and_reports_recovery_path(self):
        replace = os.replace

        def fail(source, destination):
            if Path(destination) == self.paths[-1] or Path(source).name == "restore":
                raise OSError("persistent replacement error")
            return replace(source, destination)

        with patch("output_contract.os.replace", side_effect=fail), self.assertRaisesRegex(OSError, "partial publication; rollback incomplete") as raised:
            publish_artifacts(self.artifacts)
        recovery, = self.root.glob(".basecraft-iqtree-publish-*")
        self.assertIn(str(recovery), str(raised.exception))
        manifest = json.loads((recovery / "recovery.json").read_text())
        for entry in manifest["artifacts"]:
            path = Path(entry["destination"])
            self.assertEqual(Path(entry["previous"]).read_text(), self.previous[path])
            # Demonstrate exact recovery after the injected filesystem failure clears.
            replace(entry["previous"], path)
            self.assertEqual(path.read_text(), self.previous[path])

    def test_success_replaces_complete_set_and_removes_obsolete_companion(self):
        self.artifacts[2] = (self.paths[2], None)
        publish_artifacts(self.artifacts)
        for path, content in self.artifacts:
            if content is None:
                self.assertFalse(path.exists())
            else:
                self.assertEqual(path.read_text(), content)
        self.assertEqual(list(self.root.glob(".basecraft-iqtree-publish-*")), [])


if __name__ == "__main__":
    unittest.main()
