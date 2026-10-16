"""Public allocation contract and native-engine replicate retention."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from distance_plugin import collection_path, invoke, retained_output


class AllocationContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.metadata = self.root / "allocated"
        self.metadata.mkdir()
        self.allocation = {"metadata_directory": str(self.metadata),
                           "working_directory": str(self.root), "run_id": "run-123"}

    def test_destinations_and_explicit_override(self):
        params = {"analysis_artifacts": self.allocation}
        self.assertEqual(collection_path(params), self.metadata / "bootstrap-run-123.nex")
        self.assertEqual(collection_path(dict(params, bootstrap_destination="working")),
                         self.root / "bootstrap-run-123.nex")
        self.assertEqual(collection_path({"bootstrap_trees": str(self.root / "explicit.nex")}),
                         self.root / "explicit.nex")

    def test_missing_ambiguous_invalid_and_symlink_allocation(self):
        for params in [{}, {"bootstrap_trees": "relative.nex"},
                       {"analysis_artifacts": self.allocation, "bootstrap_trees": str(self.root / "x.nex")},
                       {"analysis_artifacts": self.allocation, "bootstrap_destination": "elsewhere"}]:
            with self.subTest(params=params), self.assertRaises((ValueError, TypeError)):
                collection_path(params)
        for value in ["", "../escape", "x/y", "é", "x_y", None]:
            with self.subTest(value=value), self.assertRaises((ValueError, TypeError)):
                collection_path({"analysis_artifacts": dict(self.allocation, run_id=value)})
        symlink = self.root / "linked"
        symlink.symlink_to(self.metadata, target_is_directory=True)
        for value in ["relative", str(self.root / "missing"), str(symlink)]:
            with self.subTest(value=value), self.assertRaises((ValueError, TypeError)):
                collection_path({"analysis_artifacts": dict(self.allocation, metadata_directory=value)})
        self.assertEqual(list(self.metadata.iterdir()), [])

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
        self.target = self.root / "result.nwk"
        self.params = {"input": str(self.input), "output": str(self.target), "model": "p",
                       "threads": 1, "bootstrap": 16, "seed": 71,
                       "analysis_artifacts": self.allocation}

    def report(self):
        return json.loads(Path(str(self.target) + ".run.json").read_text())["bootstrap"]

    def test_both_destinations_discovery_and_no_duplicate_collection(self):
        for i, destination in enumerate(["metadata", "working"]):
            params = dict(self.params, bootstrap_destination=destination,
                          analysis_artifacts=dict(self.allocation, run_id=f"run-{i}"))
            result = invoke("tree", params)
            retained = self.report()["replicate_trees"]
            path = Path(retained["path"])
            self.assertEqual(path.parent, self.metadata if destination == "metadata" else self.root)
            self.assertEqual(retained["status"], "complete")
            self.assertEqual(retained["trees"], 16)
            self.assertIn(str(path), result["outputs"])
            text = path.read_text()
            self.assertEqual(text.count("TREE replicate_"), 16)
            for r in range(1, 17):
                self.assertIn(f"TREE replicate_{r} =", text)
            self.assertEqual(sum(p.read_text() == text for p in self.root.rglob("*.nex")), i + 1)
            # Only the original alignment exists; wrapper does not copy it permanently.
            self.assertEqual(list(self.root.rglob("*.fa")), [self.input])
        self.assertEqual(len(list(self.root.rglob("*.nex"))), 2)

    def test_failed_cancelled_and_disconnected_runs(self):
        self.input.write_text(">a\nA---\n>b\nAAAA\n")
        result = invoke("tree", self.params)
        report = self.report()
        self.assertTrue(report["support_withheld"])
        self.assertGreater(report["failed_replicates"], 0)
        retained = report["replicate_trees"]
        self.assertEqual(retained["status"], "partial")
        self.assertTrue(retained["path"].endswith(".partial.nex"))
        self.assertIn(retained["path"], result["outputs"])
        self.assertEqual(Path(retained["path"]).read_text().count("TREE replicate_"), report["completed"])
        cancel = self.root / "cancel"
        cancel.touch()
        params = dict(self.params, cancel_file=str(cancel),
                      analysis_artifacts=dict(self.allocation, run_id="cancelled"))
        invoke("tree", params)
        self.assertEqual(self.report()["replicate_trees"]["status"], "cancelled")
        self.input.write_text(">a\nAAA---\n>b\n---AAA\n")
        params = dict(self.params, analysis_artifacts=dict(self.allocation, run_id="disconnected"))
        with self.assertRaisesRegex(RuntimeError, "complete tree"):
            invoke("tree", params)
        self.assertEqual(self.report()["replicate_trees"]["status"], "original_failed")
        self.assertEqual(self.report()["attempted"], 0)

    def test_collisions_and_late_primary_failure_preserve_evidence(self):
        expected = collection_path(self.params)
        expected.write_text("unrelated existing collection")
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            invoke("tree", self.params)
        self.assertEqual(expected.read_text(), "unrelated existing collection")
        self.assertFalse(self.target.exists())
        expected.unlink()
        self.target.write_text("previous primary")
        with (patch("distance_plugin.os.replace", side_effect=OSError("injected publication failure")),
              self.assertRaisesRegex(RuntimeError, "retained bootstrap artifacts")):
            invoke("tree", self.params)
        self.assertEqual(self.target.read_text(), "previous primary")
        self.assertEqual(expected.read_text().count("TREE replicate_"), 16)
        self.assertEqual(len(list(self.metadata.iterdir())), 1)

    def test_failed_native_run_does_not_adopt_existing_collection(self):
        expected = collection_path(self.params)
        expected.write_text("unrelated existing collection")
        with self.assertRaises(RuntimeError) as failed:
            invoke("tree", self.params)
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
            invoke("tree", self.params)
        report = json.loads(Path(str(self.target) + ".run.json").read_text())
        self.assertEqual(report["output_status"], "failed")
        self.assertIn("scratch unavailable", report["error"])
        self.assertEqual(report["bootstrap"]["replicate_trees"]["path"], str(collection_path(self.params)))
        self.assertEqual(report["bootstrap"]["replicate_trees"]["status"], "complete")
        self.assertEqual(report["bootstrap"]["replicate_trees"]["trees"], 16)
        self.assertFalse(self.target.exists())

    def test_explicit_path_and_no_bootstrap(self):
        params = dict(self.params)
        del params["analysis_artifacts"]
        params["bootstrap_trees"] = str(self.root / "explicit.nex")
        invoke("tree", params)
        self.assertEqual(self.report()["replicate_trees"]["path"], params["bootstrap_trees"])
        params.pop("bootstrap_trees")
        params["bootstrap"] = 0
        invoke("tree", params)
        self.assertEqual(len(list(self.root.rglob("*.nex"))), 1)


if __name__ == "__main__":
    unittest.main()
