"""Native-engine replicate retention in the run's allocated artifacts."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from basecraft.plugin import local_context
from distance_plugin import invoke, retained_output


class AllocationContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()

    def test_unrelated_returned_artifact_not_adopted(self):
        unrelated = self.root / "user.nex"
        unrelated.write_text("user data")
        metadata = {"bootstrap": {"support_withheld": False, "cancelled": False,
                    "requested": 2, "attempted": 2, "completed": 2, "failed_replicates": 0, "replicate_trees": {
                        "path": str(unrelated), "format": "nexus", "status": "complete", "trees": 2}}}
        with self.assertRaisesRegex(RuntimeError, "invalid bootstrap artifact"):
            retained_output(metadata, self.root / "expected.nex")
        self.assertEqual(unrelated.read_text(), "user data")


@unittest.skipUnless(os.environ.get("BASECRAFT_DISTANCE"), "requires native distance engine")
class NativeRetention(AllocationContract):
    def setUp(self):
        super().setUp()
        self.input = self.root / "input.fa"
        self.input.write_text(">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n")
        self.target = self.artifacts / "tree.nwk"
        self.params = {"model": "p", "threads": 1, "bootstrap": 16, "seed": 71}

    def context(self, run_id="run-123"):
        return local_context(self.root, {"alignment": [str(self.input)]}, {"artifacts": "artifacts"}, run_id)

    def expected(self, run_id="run-123"):
        return self.artifacts / f"bootstrap-{run_id}.nex"

    def report(self):
        return json.loads(Path(str(self.target) + ".run.json").read_text())["bootstrap"]

    def test_replicates_are_retained_in_the_allocated_artifacts(self):
        for i in range(2):
            invoke("tree", self.params, self.context(f"run-{i}"))
            retained = self.report()["replicate_trees"]
            path = Path(retained["path"])
            self.assertEqual(path, self.expected(f"run-{i}"))
            self.assertEqual(retained["status"], "complete")
            self.assertEqual(retained["trees"], 16)
            text = path.read_text()
            self.assertEqual(text.count("TREE replicate_"), 16)
            for r in range(1, 17):
                self.assertIn(f"TREE replicate_{r} =", text)
            # Only the original alignment exists; the plugin does not copy it.
            self.assertEqual(list(self.root.rglob("*.fa")), [self.input])
        self.assertEqual(len(list(self.root.rglob("*.nex"))), 2)

    def test_failed_cancelled_and_disconnected_runs(self):
        self.input.write_text(">a\nA---\n>b\nAAAA\n")
        result = invoke("tree", self.params, self.context())
        report = self.report()
        self.assertTrue(report["support_withheld"])
        self.assertGreater(report["failed_replicates"], 0)
        retained = report["replicate_trees"]
        self.assertEqual(retained["status"], "partial")
        self.assertTrue(retained["path"].endswith(".partial.nex"))
        self.assertEqual(Path(retained["path"]).read_text().count("TREE replicate_"), report["completed"])
        described = result["execution"]["methods"]["description"]
        self.assertIn(f"{report['completed']} completed, {report['failed_replicates']} failed", described)
        self.assertIn("support was withheld", described)
        cancel = self.root / "cancel"
        cancel.touch()
        result = invoke("tree", self.params, self.context("cancelled"))
        self.assertEqual(self.report()["replicate_trees"]["status"], "cancelled")
        self.assertIn("was cancelled", result["execution"]["methods"]["description"])
        cancel.unlink()
        self.input.write_text(">a\nAAA---\n>b\n---AAA\n")
        with self.assertRaisesRegex(RuntimeError, "complete tree"):
            invoke("tree", self.params, self.context("disconnected"))
        self.assertEqual(self.report()["replicate_trees"]["status"], "original_failed")
        self.assertEqual(self.report()["attempted"], 0)

    def test_collisions_and_late_primary_failure_preserve_evidence(self):
        expected = self.expected()
        expected.write_text("unrelated existing collection")
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            invoke("tree", self.params, self.context())
        self.assertEqual(expected.read_text(), "unrelated existing collection")
        self.assertFalse(self.target.exists())
        expected.unlink()
        self.target.write_text("previous primary")
        with (patch("distance_plugin.os.replace", side_effect=OSError("injected publication failure")),
              self.assertRaisesRegex(RuntimeError, "retained bootstrap artifacts")):
            invoke("tree", self.params, self.context())
        self.assertEqual(self.target.read_text(), "previous primary")
        self.assertEqual(expected.read_text().count("TREE replicate_"), 16)
        self.assertEqual(len(list(self.artifacts.glob("*.nex"))), 1)

    def test_failed_native_run_does_not_adopt_existing_collection(self):
        expected = self.expected()
        expected.write_text("unrelated existing collection")
        with self.assertRaises(RuntimeError) as failed:
            invoke("tree", self.params, self.context())
        self.assertNotIn("retained bootstrap artifacts:", str(failed.exception))
        self.assertFalse(Path(str(self.target) + ".run.json").exists())
        self.assertEqual(expected.read_text(), "unrelated existing collection")

    def test_scratch_setup_failure_saves_failed_report_with_validated_collection(self):
        create = tempfile.NamedTemporaryFile

        def fail_primary_only(*args, **kwargs):
            if "mode" not in kwargs:
                raise OSError("scratch unavailable")
            return create(*args, **kwargs)  # Recovery report uses text-mode scratch.

        with (patch("distance_plugin.tempfile.NamedTemporaryFile", side_effect=fail_primary_only),
              self.assertRaisesRegex(RuntimeError, "retained bootstrap artifacts")):
            invoke("tree", self.params, self.context())
        report = json.loads(Path(str(self.target) + ".run.json").read_text())
        self.assertEqual(report["output_status"], "failed")
        self.assertIn("scratch unavailable", report["error"])
        self.assertEqual(report["bootstrap"]["replicate_trees"]["path"], str(self.expected()))
        self.assertEqual(report["bootstrap"]["replicate_trees"]["status"], "complete")
        self.assertEqual(report["bootstrap"]["replicate_trees"]["trees"], 16)
        self.assertFalse(self.target.exists())

    def test_no_bootstrap_writes_no_collection(self):
        invoke("tree", dict(self.params, bootstrap=0), self.context())
        self.assertEqual(list(self.root.rglob("*.nex")), [])


if __name__ == "__main__":
    unittest.main()
