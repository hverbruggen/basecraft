"""External process contract tests, without claiming mock trees as ML validation."""
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

import iqtree_plugin
from basecraft.plugin import local_context


class InputOptions(unittest.TestCase):
    def setUp(self):
        allocation = patch.object(iqtree_plugin, "allocated_cpus", return_value=1)
        allocation.start()
        self.addCleanup(allocation.stop)
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "input.fa"
        self.source.write_text(">a\nACGT\n>b\nACGT\n>c\nACGA\n>d\nTCGT\n")
        self.output = self.root / "tree.nwk"
        self.context = local_context(self.root, {"alignment": [str(self.source)]}, {"artifacts": "."})
        self.params = {"model": "JC",
                       "seed": 7, "threads": 1, "keep_identical": True, "tree_role": "display only"}

    def executable(self, body):
        path = self.root / "iqtree"
        path.write_text("#!/usr/bin/env python3\nimport sys, pathlib, time\n"
                        "if '--version' in sys.argv: print('test double'); sys.exit(0)\n" + body)
        path.chmod(0o755)
        return patch.object(iqtree_plugin, "executable", return_value=str(path))

    def test_identity_parameters_and_display_units(self):
        with self.executable("prefix = pathlib.Path(sys.argv[sys.argv.index('-pre')+1])\n"
                             "prefix.with_suffix('.treefile').write_text('((t0:1,t1:1):1,(t2:1,t3:1):1);')\n"):
            result = iqtree_plugin.tree(self.params, self.context)
        self.assertEqual(self.output.read_text(), "(('a':1,'b':1):1,('c':1,'d':1):1);\n")
        effective = result["execution"]["resolved_parameters"]
        self.assertEqual(effective["units"], "substitutions per site")
        self.assertEqual(effective["tree_role"], "display only")
        self.assertEqual(effective["seed"], 7)
        self.assertIn("-keep-ident", result["execution"]["argv"])
        report = json.loads(Path(str(self.output) + ".run.json").read_text())
        self.assertEqual(set(report["identifiers"].values()), {"a", "b", "c", "d"})

    def test_support_options_reach_iqtree(self):
        tree = "prefix = pathlib.Path(sys.argv[sys.argv.index('-pre')+1])\nprefix.with_suffix('.treefile').write_text('((t0:1,t1:1)95:1,(t2:1,t3:1)88:1);')\n"
        for support, replicates, flag in (("ufboot", 1000, "-B"), ("bootstrap", 100, "-b")):
            with self.executable(tree):
                result = iqtree_plugin.tree(dict(self.params, model="GTR+I+G", support=support, replicates=replicates), self.context)
            argv = result["execution"]["argv"]
            self.assertEqual(argv[argv.index(flag) + 1], str(replicates))
            self.assertEqual(argv[argv.index("-m") + 1], "GTR+I+G")
            self.assertIn(str(replicates), result["execution"]["resolved_parameters"]["support"])
        self.assertIn(")95:1", self.output.read_text())
        # A base model takes its rate options from the form's switches.
        with self.executable(tree):
            result = iqtree_plugin.tree(dict(self.params, model="HKY", gamma=True, invariant=True), self.context)
        argv = result["execution"]["argv"]
        self.assertEqual(argv[argv.index("-m") + 1], "HKY+I+G")
        # Standard bootstrap trees go where the run's destination says, named.
        boot = tree + "prefix.with_suffix('.boottrees').write_text('((t0:1,t1:1):1,t2:1);\\n((t0:1,t2:1):1,(t1:1,t3:1):1);\\n')\n"
        with self.executable(boot):
            result = iqtree_plugin.tree(dict(self.params, support="bootstrap", replicates=2), self.context)
        trees = self.root / "bootstrap-local.nex"
        text = trees.read_text()
        # Quoted as in the main tree file.
        self.assertIn("tree replicate_2 = (('a':1,'c':1):1,('b':1,'d':1):1);", text)
        self.assertIn("tree replicate_1 = (('a':1,'b':1):1,'c':1);", text)
        with self.assertRaisesRegex(ValueError, "1000"):
            iqtree_plugin.tree(dict(self.params, support="ufboot", replicates=100), self.context)

    def test_missing_or_duplicate_labels_preserve_previous_output(self):
        for tree in ("(t0:1,t1:1,t2:1);", "(t0:1,t1:1,t2:1,t2:1);", "(t0:1,t1:1,t2:1,t3:1,rogue:1);"):
            self.output.write_text("previous tree")
            with self.executable("prefix = pathlib.Path(sys.argv[sys.argv.index('-pre')+1])\n"
                                 f"prefix.with_suffix('.treefile').write_text({tree!r})\n"), self.assertRaisesRegex(RuntimeError, "specimen labels"):
                iqtree_plugin.tree(self.params, self.context)
            self.assertEqual(self.output.read_text(), "previous tree")

    def test_sidecar_directory_preserves_previous_tree(self):
        self.output.write_text("previous tree\n")
        Path(str(self.output) + ".run.json").mkdir()
        with self.executable("prefix = pathlib.Path(sys.argv[sys.argv.index('-pre')+1])\n"
                             "prefix.with_suffix('.treefile').write_text('(t0:1,t1:1,t2:1,t3:1);')\n"), self.assertRaisesRegex(OSError, "previous result preserved"):
            iqtree_plugin.tree(self.params, self.context)
        self.assertEqual(self.output.read_text(), "previous tree\n")
        self.assertTrue(Path(str(self.output) + ".run.json").is_dir())

    def test_cancellation_stops_process_without_output(self):
        cancel = self.root / "cancel"
        timer = threading.Timer(0.2, lambda: cancel.write_text("cancel"))
        with self.executable("time.sleep(30)\n"):
            timer.start()
            try:
                with self.assertRaisesRegex(RuntimeError, "cancelled"):
                    iqtree_plugin.tree(self.params, self.context)
            finally:
                timer.join()
        self.assertFalse(self.output.exists())

    def test_missing_tool_and_pre_cancel_fail_usefully(self):
        with patch.object(iqtree_plugin, "executable", side_effect=FileNotFoundError("Install IQ-TREE")), self.assertRaisesRegex(FileNotFoundError, "Install IQ-TREE"):
            iqtree_plugin.tree(self.params, self.context)
        cancel = self.root / "cancel"
        cancel.touch()
        with self.assertRaisesRegex(RuntimeError, "cancelled before inference"):
            iqtree_plugin.tree(self.params, self.context)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
