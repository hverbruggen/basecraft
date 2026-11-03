"""Producer and accepted-input identity checks used by registered cache bindings."""
import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path

from distance_plugin import invoke


@unittest.skipUnless(os.environ.get("BASECRAFT_DISTANCE"), "requires native engine")
class RegisteredCacheIdentity(unittest.TestCase):
    def test_changed_source_and_same_version_different_producer_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.afa"
            source.write_text(">a\nAAAAAC\n>b\nAAAACC\n>c\nAAACCC\n")
            original = source.read_bytes()
            target = root / "matrix.tsv"
            params = {"input": str(source), "output": str(target), "model": "p"}
            invoke("matrix", params)
            cache = Path(str(target) + ".distances.bin")
            saved = cache.read_bytes()
            params.update(output=str(root / "next.tsv"), cache_input=str(cache))
            invoke("matrix", params)
            self.assertTrue(json.loads((root / "next.tsv.run.json").read_text())["cache_reused"])
            source.write_text(">a\nAAAAAT\n>b\nAAAACC\n>c\nAAACCC\n")
            with self.assertRaisesRegex(ValueError, "producer do not match"):
                invoke("matrix", dict(params, output=str(root / "changed.tsv")))
            source.write_bytes(original)
            other = root / "other-engine"
            shutil.copyfile(os.environ["BASECRAFT_DISTANCE"], other)
            # An identical advertised version is insufficient for producer identity.
            with other.open("ab") as stream:
                stream.write(b"different producer bytes")
            with self.assertRaisesRegex(ValueError, "producer do not match"):
                invoke("matrix", dict(params, executable=str(other), output=str(root / "other.tsv")))
            self.assertEqual(cache.read_bytes(), saved)
            self.assertFalse((root / "changed.tsv").exists())
            self.assertFalse((root / "other.tsv").exists())
