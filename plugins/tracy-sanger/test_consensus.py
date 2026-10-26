"""Safety and evidence tests for the public Tracy plugin wrapper."""

import tempfile
import unittest
from pathlib import Path

from consensus import evidence, merge


class ConsensusTests(unittest.TestCase):
    def test_more_than_three_is_refused_before_tool_access(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "draft"
            with self.assertRaisesRegex(ValueError, "Exactly 1, 2 or 3"):
                merge([Path("missing.ab1")] * 4, Path("missing-tracy"), target)
            self.assertFalse(target.exists())

    def test_duplicate_content_is_not_independent_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / f"read_{i}.ab1" for i in (1, 2)]
            for path in paths:
                path.write_bytes(b"ABIFsame-data")
            with self.assertRaisesRegex(ValueError, "Duplicate trace content"):
                merge(paths, Path("missing-tracy"), Path(directory) / "draft")

    def test_content_addressed_abif_is_not_rejected_for_its_filename(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "preserved-input"
            source.write_bytes(b"ABIFsynthetic-header")
            with self.assertRaisesRegex(ValueError, "Tracy executable not found"):
                merge([source], Path("basecraft-nonexistent-tracy"), Path(directory) / "draft")

    def test_every_read_must_join_a_connected_overlap(self):
        rows = [
            ("read_1", "AAAAA-----"),
            ("read_2", "AAAAATTTTT"),
            ("read_3", "-----GGGGG"),
        ]
        with self.assertRaisesRegex(ValueError, "connected"):
            evidence(rows, "AAAAATTTTT", minimum_overlap=3, minimum_identity=0.8)

    def test_terminal_absence_is_not_an_internal_gap(self):
        rows = [("read_1", "-AC-GT"), ("read_2", "TACCG-")]
        result = evidence(rows, "TACCGT", minimum_overlap=3, minimum_identity=0.8)
        self.assertEqual(
            result["columns"][0]["support"], [{"read": 2, "base": "T", "kind": "call"}]
        )
        self.assertEqual(result["columns"][3]["support"][0]["kind"], "internal_gap")
        self.assertTrue(result["columns"][3]["conflict"])


if __name__ == "__main__":
    unittest.main()
