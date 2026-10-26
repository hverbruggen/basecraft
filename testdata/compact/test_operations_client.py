"""Client transport regressions without a compiled host or schema interpretation."""
import io
from pathlib import Path
import sys
import threading
import unittest
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python/src"))
from basecraft.operations import OperationError, RunHandle  # noqa: E402


class OperationClientTests(unittest.TestCase):
    def handle(self, response):
        handle = RunHandle.__new__(RunHandle)
        handle._lock = threading.Lock()
        handle.job = "owned-job"
        handle._process = SimpleNamespace(stdin=io.BytesIO(), stdout=io.BytesIO(response))
        return handle

    def test_host_outcomes_are_not_reinterpreted(self):
        handle = self.handle(b'{"state":"completed","data":{"answer":0}}\n')
        self.assertEqual(handle.status(), {"state": "completed", "data": {"answer": 0}})
        self.assertIn(b'"job": "owned-job"', handle._process.stdin.getvalue())

    def test_closed_malformed_and_nonobject_responses_are_typed(self):
        for response in [b"", b"not json\n", b"[]\n"]:
            with self.subTest(response=response), self.assertRaises(OperationError) as raised:
                self.handle(response).status()
            self.assertEqual(raised.exception.code, "plugin_protocol_violation")
            self.assertEqual(raised.exception.run_id, "owned-job")

    def test_broken_session_is_a_typed_error(self):
        handle = self.handle(b"")
        handle._process.stdin.close()
        with self.assertRaises(OperationError) as raised:
            handle.cancel()
        self.assertEqual(raised.exception.code, "plugin_protocol_violation")


if __name__ == "__main__":
    unittest.main()
