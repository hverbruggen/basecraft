"""Python MCP session client against a fake server process; no compiled host."""
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import time
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python/src"))
from basecraft import operations  # noqa: E402
from basecraft.session import OperationError, Session, tool_name  # noqa: E402

FAKE = Path(__file__).with_name("fake_mcp_server.py")
OPERATION = "org.example.fixture/count@1"


class SessionTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.log = self.root / "calls.jsonl"
        self.command = self.root / "basecraft"
        self.command.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{FAKE}" "$@"\n')
        self.command.chmod(self.command.stat().st_mode | stat.S_IXUSR)
        self.environment(FAKE_MCP_LOG=str(self.log), FAKE_MCP_MODE="normal")

    def environment(self, **values):
        patch = mock.patch.dict(os.environ, values)
        patch.start()
        self.addCleanup(patch.stop)

    def session(self, **arguments):
        session = Session(self.root, command=str(self.command), **arguments)
        self.addCleanup(session.close)
        return session

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_discovery_follows_pages_and_keeps_schemas(self):
        session = self.session()
        self.assertEqual(len(session.tools()), 5)
        [listed] = session.operations()
        self.assertEqual(listed["operation"], OPERATION)
        self.assertEqual(listed["tool"], tool_name(OPERATION))
        self.assertFalse(
            listed["input_schema"]["properties"]["parameters"]["additionalProperties"])
        self.assertEqual(listed["pins"], {"generation": "g"})

    def test_read_and_read_only_write_denial(self):
        session = self.session()
        document = session.open("gene.fasta")
        self.assertEqual(session.read(document["document"], 1, 3), {"residues": "CG"})
        with self.assertRaises(OperationError) as raised:
            session.apply(document["document"], 0, {"op": "seq.replace"})
        self.assertEqual(raised.exception.code, "permission_denied")
        self.assertIn("read-only", str(raised.exception))
        session.close()
        self.assertNotIn("--allow-writes", self.calls()[-1]["argv"])

    def test_apply_and_save_in_write_mode(self):
        session = self.session(writes=True, client="lab-script")
        document = session.open("gene.fasta")
        edited = session.apply(0, document["version"], {"op": "seq.replace"})
        self.assertEqual(edited["version"], 1)
        with self.assertRaises(OperationError) as raised:
            session.apply(0, 0, {"op": "seq.replace"})  # Stale version is refused.
        self.assertEqual(raised.exception.code, "stale_revision")
        self.assertEqual(session.save(0, "copy.fasta")["saved"], "copy.fasta")
        session.close()
        self.assertEqual(self.calls()[-1]["argv"][-5:], ["--client-kind", "script", "--client",
                                                         "lab-script", "--allow-writes"])

    def test_parameter_validation_error_keeps_host_code(self):
        session = self.session()
        with self.assertRaises(OperationError) as raised:
            session.submit(OPERATION, parameters={"unknown": True})
        self.assertEqual(raised.exception.code, "invalid_arguments")
        self.assertEqual(raised.exception.outcome["operation"], OPERATION)

    def test_unknown_and_malformed_operations(self):
        session = self.session()
        with self.assertRaises(OperationError) as raised:
            session.submit("org.example.fixture/missing@1")
        self.assertEqual(raised.exception.code, "plugin_unavailable")
        # The host's structured error.data, not a locally written message.
        self.assertEqual(raised.exception.outcome["operation"], "org.example.fixture/missing@1")
        self.assertIn("unknown operation", str(raised.exception))
        self.assertEqual(raised.exception.rpc_code, -32602)
        with self.assertRaises(OperationError) as raised:
            session.submit("not-an-operation")
        self.assertEqual(raised.exception.code, "invalid_arguments")

    def test_unknown_operation_without_error_data(self):
        self.environment(FAKE_MCP_MODE="bare-unknown")
        with self.assertRaises(OperationError) as raised:
            self.session().submit("org.example.fixture/missing@1")
        self.assertEqual(raised.exception.code, "plugin_unavailable")
        self.assertIn("Unknown operation", str(raised.exception))

    def test_job_result(self):
        session = self.session()
        outcome = session.run(OPERATION, parameters={"mode": "normal"}, timeout=10)
        self.assertEqual(outcome["data"], {"bytes": 7})
        self.assertIn("basecraft.host.jobs_result.v1", [c.get("tool") for c in self.calls()])

    def test_wait_timeout_does_not_cancel_and_cancel_is_explicit(self):
        session = self.session()
        job = session.submit(OPERATION, parameters={"mode": "slow"})
        with self.assertRaises(TimeoutError):
            job.result(timeout=0.05)
        self.assertEqual(job.status()["state"], "running")
        self.assertEqual(job.cancel()["state"], "cancelled")
        with self.assertRaises(OperationError) as raised:
            job.result()
        self.assertEqual(raised.exception.run_id, job.job)

    def test_close_cancels_owned_unfinished_jobs(self):
        with Session(self.root, command=str(self.command)) as session:
            job = session.submit(OPERATION, parameters={"mode": "slow"})
            finished = session.submit(OPERATION, parameters={"mode": "normal"})
            finished.result(timeout=10)
        calls = self.calls()
        cancelled = [c["arguments"]["job"] for c in calls
                     if c.get("tool") == "basecraft.host.jobs_cancel.v1"]
        self.assertEqual(cancelled, [job.job])
        self.assertEqual(calls[-1]["eof"], [])

    def test_request_timeout_skips_the_late_reply(self):
        self.environment(FAKE_MCP_MODE="late")
        session = self.session(timeout=0.1)
        session.open("gene.fasta")
        with self.assertRaises(TimeoutError):
            session.read(0, 0, 4)
        time.sleep(0.6)
        self.assertEqual(session.open("gene.fasta")["document"], 0)

    def test_transport_failures_are_typed(self):
        for mode in ["oversize", "garbage", "array"]:
            with self.subTest(mode=mode):
                self.environment(FAKE_MCP_MODE=mode)
                session = self.session()
                with self.assertRaises(OperationError) as raised:
                    session.read(0, 0, 4)
                self.assertEqual(raised.exception.code, "plugin_protocol_violation")

    def test_startup_failure_reports_host_stderr(self):
        self.environment(FAKE_MCP_MODE="crash")
        with self.assertRaises(OperationError) as raised:
            Session(self.root, command=str(self.command))
        self.assertEqual(raised.exception.code, "plugin_protocol_violation")
        self.assertIn("project folder does not exist", str(raised.exception))

    def test_operations_module_uses_a_writable_session(self):
        self.environment(BASECRAFT=str(self.command))
        outcome = operations.run(OPERATION, project=self.root, parameters={"mode": "normal"})
        self.assertEqual(outcome["data"], {"bytes": 7})
        with operations.submit(OPERATION, project=self.root,
                               parameters={"mode": "slow"}) as handle:
            self.assertEqual(handle.status()["state"], "running")
        eof = [c for c in self.calls() if "eof" in c]
        self.assertTrue(all("--allow-writes" in c["argv"] for c in eof))
        self.assertEqual(eof[-1]["eof"], [])
        with self.assertRaises(OperationError) as raised:
            operations.run(OPERATION, project=self.root, parameters={"unknown": 1})
        self.assertEqual(raised.exception.code, "invalid_arguments")


if __name__ == "__main__":
    unittest.main()
