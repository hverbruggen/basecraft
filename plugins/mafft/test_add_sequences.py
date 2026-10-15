"""Exercise real MAFFT addition, including loss detection and orientation."""

import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from add_sequences import run_add


class AdditionTests(unittest.TestCase):
    def setUp(self):
        self.mafft = os.environ.get("BASECRAFT_MAFFT") or shutil.which("mafft")
        if not self.mafft:
            self.skipTest("MAFFT is unavailable")

    def test_insertions_are_reported_then_retained_when_gaps_allowed(self):
        sequence = "ACGTTAGCCTAGGCTAACGTTGACCTGATCGTACGATCGT"
        reference = sequence[:12] + "--" + sequence[12:]
        novel = sequence[:20] + "AAAAA" + sequence[20:]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "ref.fa"
            source.write_text(f">a\n{reference}\n>b\n{reference}\n")
            before = source.read_bytes()
            frozen = run_add(
                source, [("new", novel)], root / "frozen.json", self.mafft, keep=True
            )
            self.assertEqual(frozen["insertions"], 5)
            self.assertEqual(frozen["reference"], [reference, reference])
            expanded = run_add(
                source, [("new", novel)], root / "expanded.json", self.mafft, keep=False
            )
            self.assertEqual(expanded["rows"][0][1].replace("-", ""), novel)
            self.assertGreater(len(expanded["reference"][0]), len(reference))
            self.assertEqual(expanded["reference"][0].replace("-", ""), sequence)
            self.assertEqual(source.read_bytes(), before)

    def test_reverse_complement_and_multiple_additions(self):
        sequence = "ACGTTAGCCTAGGCTAACGTTGACCTGATCGTACGATCGT"
        reverse = sequence.translate(str.maketrans("ACGT", "TGCA"))[::-1]
        with (
            tempfile.TemporaryDirectory() as directory,
            patch.dict(os.environ, {"BASECRAFT_CPU_THREADS": "1"}),
        ):
            root = Path(directory)
            source = root / "ref.fa"
            source.write_text(f">a\n{sequence}\n>b\n{sequence}\n")
            result = run_add(
                source,
                [("reverse", reverse), ("forward", sequence)],
                root / "out.json",
                self.mafft,
                orientation=True,
                threads=8,
            )
            self.assertEqual(
                result["parameters"][result["parameters"].index("--thread") + 1], "1"
            )
            self.assertEqual(result["insertions"], 0)
            self.assertEqual(result["reversed"], ["reverse"])
            self.assertEqual([seq for _, seq in result["rows"]], [sequence, sequence])


if __name__ == "__main__":
    unittest.main()
