"""Late failures of the native distance engine leave a usable result."""
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from basecraft.plugin import local_context
from distance_plugin import invoke


@unittest.skipUnless(os.environ.get("BASECRAFT_DISTANCE"), "requires native engine")
class RetentionRecoveryReview(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        source = self.root / "input.fa"
        source.write_text(">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n")
        (self.root / "artifacts").mkdir()
        self.context = local_context(self.root, {"alignment": [str(source)]}, {"artifacts": "artifacts"})
        self.collection = self.root / "artifacts" / "bootstrap-local.nex"
        self.target = self.root / "artifacts" / "tree.nwk"
        self.params = {"model": "p", "threads": 1, "bootstrap": 4}

    def test_primary_scratch_creation_failure_reports_retained_collection(self):
        with (patch("distance_plugin.tempfile.NamedTemporaryFile", side_effect=OSError("injected scratch creation failure")),
              self.assertRaises((RuntimeError, OSError)) as failure):
            invoke("tree", self.params, self.context)
        self.assertTrue(self.collection.is_file())
        self.assertIn(str(self.collection), str(failure.exception))

    def test_late_report_failure_reports_retained_collection(self):
        with (patch("distance_plugin.write_atomic", side_effect=OSError("injected report failure")),
              self.assertRaises(RuntimeError) as failure):
            invoke("tree", self.params, self.context)
        self.assertTrue(self.collection.is_file())
        self.assertTrue(self.target.is_file())
        self.assertIn(str(self.collection), str(failure.exception))


if __name__ == "__main__":
    unittest.main()
