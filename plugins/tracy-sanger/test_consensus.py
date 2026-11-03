"""Safety and evidence tests for the public Tracy plugin wrapper."""

import copy
import io
import json
import subprocess
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from consensus import evidence, main, merge, trace_calls


class ConsensusTests(unittest.TestCase):
    def test_cli_reports_malformed_tool_output_without_traceback(self):
        message = io.StringIO()
        with (
            patch("sys.argv", ["consensus", "read.ab1", "--output-dir", "unused"]),
            patch("consensus.merge", side_effect=TypeError("Invalid Tracy call evidence")),
            patch("sys.stderr", message),
            self.assertRaises(SystemExit) as failure,
        ):
            main()
        self.assertEqual(failure.exception.code, 2)
        self.assertEqual(message.getvalue(), "Sanger consensus refused: Invalid Tracy call evidence\n")

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


class NativeCallsTests(unittest.TestCase):
    @staticmethod
    def raw():
        # Synthetic event-aligned C/T, A/A, T/T controls. Not a peak classifier.
        return {
            "primarySeq": "CAT", "secondarySeq": "TAT",
            "basecallPos": [2, 4, 6], "basecallQual": [40, 46, 43],
            "pos": list(range(1, 8)),
            **{"peak" + base: [100] * 7 for base in "ACGT"},
        }

    def normalize(self, raw):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / "calls.json"
            output.write_text(json.dumps(raw))
            with patch("consensus.subprocess.run", return_value=subprocess.CompletedProcess([], 0)):
                return trace_calls(Path("read.ab1"), Path("tracy"), output)

    def test_native_control_events_and_legacy_absence(self):
        raw = self.raw()
        result = self.normalize(raw)
        self.assertEqual(result, {
            "sequence": "CAT", "secondary_sequence": "TAT",
            "positions": [1, 3, 5], "scores": [40, 46, 43],
            "signal_bounds": [0, 3],
        })
        for secondary in (None, "missing"):
            legacy = copy.deepcopy(raw)
            if secondary == "missing":
                del legacy["secondarySeq"]
            else:
                legacy["secondarySeq"] = None
            absent = self.normalize(legacy)
            self.assertIsNone(absent.pop("secondary_sequence"))
            self.assertEqual(absent, {k: v for k, v in result.items() if k != "secondary_sequence"})
        raw["secondarySeq"] = "ryk"
        self.assertEqual(self.normalize(raw)["secondary_sequence"], "RYK")

    def test_malformed_evidence_is_refused(self):
        for raw in (None, [], "not an object", 7):
            with self.subTest(raw=raw), self.assertRaises(TypeError):
                self.normalize(raw)
        for key, value in (
            ("secondarySeq", "TA"), ("secondarySeq", "T-T"),
            ("secondarySeq", ""), ("secondarySeq", ["T", "A", "T"]),
            ("primarySeq", ""), ("primarySeq", "C?T"),
            ("basecallPos", [0, 4, 6]), ("basecallPos", [2, 2, 6]),
            ("basecallPos", [2, 4, 8]), ("basecallPos", [2, 4.5, 6]),
            ("basecallPos", [True, 4, 6]), ("basecallPos", []),
            ("basecallQual", [40, 46]), ("basecallQual", [40, "46", 43]),
            ("basecallQual", [40, True, 43]), ("basecallQual", [40, 2**31, 43]),
            ("peakA", []), ("peakC", None), ("pos", None),
        ):
            with self.subTest(key=key, value=value):
                raw = self.raw()
                raw[key] = value
                with self.assertRaises(ValueError):
                    self.normalize(raw)

    def test_single_and_reverse_paired_artifacts_preserve_native_arrays(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            engine = root / "tracy"
            engine.write_bytes(b"synthetic engine identity")
            reads = [root / "one.ab1", root / "two.ab1"]
            for i, read in enumerate(reads):
                read.write_bytes(b"ABIF" + bytes([i]))

            rendezvous = None
            outputs = set()
            output_lock = threading.Lock()
            def run(argv, **kwargs):
                if argv[1] == "basecall":
                    if rendezvous is not None:
                        # Fails if the pair was serialized. Unique outputs and
                        # input ordering must survive concurrent completion.
                        with output_lock:
                            output = argv[argv.index("-o") + 1]
                            self.assertNotIn(output, outputs)
                            outputs.add(output)
                        rendezvous.wait(timeout=5)
                    raw = self.raw()
                    if argv[-1] == str(reads[1]):
                        raw["primarySeq"], raw["secondarySeq"] = "ATG", "ATA"
                    Path(argv[argv.index("-o") + 1]).write_text(json.dumps(raw))
                elif argv[1] == "consensus":
                    work = Path(kwargs["cwd"])
                    # Deliberately reverse row order; native evidence follows input identity.
                    (work / "result.align.fa").write_text(">read_2 (reverse)\nCAT\n>read_1\nCAT\n")
                    (work / "result.fa").write_text(">consensus\nCAT\n")
                return subprocess.CompletedProcess(argv, 0, "synthetic", "")

            with patch("consensus.shutil.which", return_value=str(engine)), patch("consensus.subprocess.run", side_effect=run):
                for count, workers in ((1, 1), (2, 1), (2, 2)):
                    rendezvous = threading.Barrier(2) if workers == 2 else None
                    destination = root / f"{count}-{workers}"
                    result = merge(reads[:count], engine, destination, 1, workers=workers)
                    self.assertEqual(result["consensus"], "CAT")
                    self.assertEqual(result["conflict_columns"], 0)
                    self.assertEqual(result["inputs"][0]["calls"]["secondary_sequence"], "TAT")
                    if count == 2:
                        reverse = result["inputs"][1]
                        self.assertEqual(reverse["orientation"], "reverse")
                        self.assertEqual(reverse["calls"]["sequence"], "ATG")
                        self.assertEqual(reverse["calls"]["secondary_sequence"], "ATA")
                        self.assertEqual(reverse["calls"]["positions"], [1, 3, 5])
                    stored = json.loads((destination / "evidence.json").read_text())
                    self.assertEqual(stored, result)
                self.assertEqual(
                    json.loads((root / "2-1" / "evidence.json").read_text()),
                    json.loads((root / "2-2" / "evidence.json").read_text()),
                    "Concurrent basecalling must preserve scientific evidence exactly",
                )


if __name__ == "__main__":
    unittest.main()
