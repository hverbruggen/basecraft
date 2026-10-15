"""Sanger placement preserves source columns and novel insertion evidence."""

import hashlib
import os
import shutil
import tempfile
import unittest
from pathlib import Path

from sanger_place import place


class SangerPlacementTests(unittest.TestCase):
    def test_fragment_map_keeps_frozen_alignment_and_insertions(self):
        mafft = os.environ.get("BASECRAFT_MAFFT") or shutil.which("mafft")
        if not mafft:
            self.skipTest("MAFFT executable unavailable")
        original = "ACGTTAGCCTAGGCTAACGTTGACCTGATCGTACGATCGT"
        variant = original[:14] + "A" + original[15:]
        fragment = original[5:25]
        fragment = fragment[:8] + "AAA" + fragment[8:]
        with tempfile.TemporaryDirectory() as directory:
            alignment = Path(directory) / "source.fa"
            output = Path(directory) / "placement.json"
            alignment.write_text(f">a\n{original}\n>b\n{variant}\n")
            before = hashlib.sha256(alignment.read_bytes()).hexdigest()
            result = place(alignment, fragment, output, mafft)
            self.assertEqual(result["source_sha256"], before)
            self.assertEqual(hashlib.sha256(alignment.read_bytes()).hexdigest(), before)
            self.assertEqual(result["insertions"], 3)
            self.assertEqual(len(result["map"]), len(fragment))
            self.assertEqual(result["source_width"], len(original))
            self.assertTrue(output.exists())

    def test_invalid_fragment_refused_before_output(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "placement.json"
            with self.assertRaisesRegex(ValueError, "IUPAC"):
                place(Path("missing.fa"), "A!C", target, "missing-mafft")
            self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()
