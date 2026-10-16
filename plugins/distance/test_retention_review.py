"""Independent late-failure recovery probes against the native distance engine."""
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from distance_plugin import invoke


@unittest.skipUnless(os.environ.get("BASECRAFT_DISTANCE"), "requires native engine")
class RetentionRecoveryReview(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        source = self.root / "input.fa"
        source.write_text(">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n")
        self.collection = self.root / "trees.nex"
        self.target = self.root / "primary.nwk"
        self.params = {"input": str(source), "output": str(self.target), "model": "p",
                       "threads": 1, "bootstrap": 4, "bootstrap_trees": str(self.collection)}

    def test_native_cache_failure_reports_retained_collection(self):
        cache_dir = self.root / "read-only"
        cache_dir.mkdir()
        cache_dir.chmod(0o555)
        try:
            with self.assertRaises(RuntimeError) as failure:
                invoke("tree", dict(self.params, cache=str(cache_dir / "cache.json")))
        finally:
            cache_dir.chmod(0o755)
        self.assertTrue(self.collection.is_file())
        self.assertIn(str(self.collection), str(failure.exception))

    def test_primary_scratch_creation_failure_reports_retained_collection(self):
        with (patch("distance_plugin.tempfile.NamedTemporaryFile", side_effect=OSError("injected scratch creation failure")),
              self.assertRaises((RuntimeError, OSError)) as failure):
            invoke("tree", self.params)
        self.assertTrue(self.collection.is_file())
        self.assertIn(str(self.collection), str(failure.exception))

    def test_late_report_failure_reports_retained_collection(self):
        with (patch("distance_plugin.write_atomic", side_effect=OSError("injected report failure")),
              self.assertRaises(RuntimeError) as failure):
            invoke("tree", self.params)
        self.assertTrue(self.collection.is_file())
        self.assertTrue(self.target.is_file())
        self.assertIn(str(self.collection), str(failure.exception))


if __name__ == "__main__":
    unittest.main()
