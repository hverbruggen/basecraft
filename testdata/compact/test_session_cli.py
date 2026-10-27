"""Python session against a real `basecraft mcp` server.

Skipped unless BASECRAFT names a built CLI. Uses an isolated profile and the
registry fixture plugin; the interpreter running this test must have pyarrow:
  BASECRAFT=/abs/target/debug/basecraft python3 -m unittest testdata/compact/test_session_cli.py
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "python/src"))
from basecraft.session import OperationError, Session  # noqa: E402

OPERATION = "org.example.registry-fixture/count@1"
INPUTS = {"sources": [{"kind": "file", "path": "a.txt"}, {"kind": "file", "path": "b.txt"}]}


@unittest.skipUnless(os.environ.get("BASECRAFT"), "set BASECRAFT to a built basecraft CLI")
class RealSessionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        root = Path(cls.directory.name)
        cls.project = root / "project"
        cls.project.mkdir()
        (cls.project / "a.txt").write_bytes(b"abc\n")
        (cls.project / "b.txt").write_bytes(b"xy\n")
        (cls.project / "gene.fasta").write_text(">gene\nACGT\n")
        cls.patch = mock.patch.dict(os.environ, {
            "BASECRAFT_PROFILE_DIR": str(root / "profile"),
            "BASECRAFT_PLUGINS": str(root / "empty-bundle"),
            "BASECRAFT_PYTHON": sys.executable})
        cls.patch.start()
        package = root / "package"
        shutil.copytree(REPO / "testdata/fixtures/registry-plugin", package)
        subprocess.run([os.environ["BASECRAFT"], "plugins", "install", str(package)],
                       check=True, capture_output=True, timeout=60)

    @classmethod
    def tearDownClass(cls):
        cls.patch.stop()
        cls.directory.cleanup()

    def session(self, **arguments):
        session = Session(self.project, **arguments)
        self.addCleanup(session.close)
        return session

    def test_read_only_session_reads_and_refuses_writes(self):
        session = self.session()
        names = {tool["name"] for tool in session.tools()}
        self.assertTrue({"open", "read", "apply", "save", "document_record"} <= names)
        self.assertNotIn(OPERATION, [item["operation"] for item in session.operations()])
        document = session.open("gene.fasta")
        self.assertEqual(session.read(document["document"], 0, 4), {"residues": "ACGT"})
        with self.assertRaises(OperationError) as raised:
            session.apply(document["document"], document["version"],
                          {"op": "seq.replace", "params": {"start": 0, "end": 1, "residues": "T"}})
        self.assertIn("read-only", str(raised.exception))
        with self.assertRaises(OperationError) as raised:
            session.submit(OPERATION, inputs=INPUTS, destinations={"report": "denied.json"})
        self.assertEqual(raised.exception.code, "permission_denied")
        self.assertFalse((self.project / "denied.json").exists())

    def test_write_session_edits_saves_and_records(self):
        (self.project / "edit.fasta").write_text(">gene\nACGT\n")
        session = self.session(writes=True)
        document = session.open("edit.fasta")
        edited = session.apply(document["document"], document["version"],
                               {"op": "seq.replace", "params": {"start": 0, "end": 1, "residues": "T"}})
        self.assertTrue(edited["can_undo"])
        self.assertTrue(session.document_record(document["document"])["unsaved"])
        session.save(document["document"])
        self.assertIn("TCGT", (self.project / "edit.fasta").read_text())

    def test_operations_validate_run_and_cancel(self):
        session = self.session(writes=True)
        listed = {item["operation"]: item for item in session.operations()}
        self.assertFalse(
            listed[OPERATION]["input_schema"]["properties"]["parameters"]["additionalProperties"])
        with self.assertRaises(OperationError) as raised:
            session.submit(OPERATION, inputs=INPUTS, parameters={"unknown_setting": True},
                           destinations={"report": "invalid.json"})
        self.assertEqual(raised.exception.code, "invalid_arguments")
        outcome = session.run(OPERATION, inputs=INPUTS, parameters={"mode": "normal"},
                              destinations={"report": "count.json"}, timeout=60)
        self.assertEqual(outcome["data"], {"bytes": 7})
        slow = session.submit(OPERATION, inputs=INPUTS, parameters={"mode": "slow"},
                              destinations={"report": "slow.json"})
        with self.assertRaises(TimeoutError):
            slow.result(timeout=0.1)
        slow.cancel()
        with self.assertRaises(OperationError) as raised:
            slow.result(timeout=60)
        self.assertEqual(raised.exception.outcome["state"], "cancelled")
        self.assertFalse((self.project / "slow.json").exists())

    def test_close_cancels_owned_job(self):
        with Session(self.project, writes=True) as session:
            session.submit(OPERATION, inputs=INPUTS, parameters={"mode": "hang"},
                           destinations={"report": "hang.json"})
        self.assertFalse((self.project / "hang.json").exists())


if __name__ == "__main__":
    unittest.main()
