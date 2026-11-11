"""Transport contract tests: synthetic data only, never contact NCBI."""
import hashlib
import json
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import blast_plugin as blast
from basecraft.plugin import local_context


class Submission(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        fasta = ">query_1\nACGT\n>query_2\nNNNN\n"
        self.plan = {"schema": 1, "fasta": fasta, "query_hash": "sha256:" + hashlib.sha256(fasta.encode()).hexdigest(),
                         "approval_hash": "frozen", "alphabet": "nucleotide", "program": "blastn", "database": "nt",
                         "oversized": False, "truncation": False, "submitted_residues": 8, "records": []}
        self.source = self.root / "query.json"
        self.source.write_text(json.dumps(self.plan))
        self.context = local_context(self.root, {"queries": [str(self.source)]}, {"receipt": "receipt"})
        self.params = {"contact_email": "test@example.org", "expect": 0.05, "max_hits": 100, "megablast": False, "truncate": False}
        self.calls = []

    def transport(self, fields):
        self.calls.append(fields)
        return "QBlastInfoBegin\n    RID = ABCD1234567\n    RTOE = 8\nQBlastInfoEnd"

    def test_acceptance_exact_fields_and_retained_identity(self):
        result = blast.submit_prepared(self.params, self.context, self.transport)
        data = result["data"]
        self.assertEqual(data["status"], "accepted")
        self.assertEqual(json.loads((self.root / "receipt").read_text()), data)
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls[0]["NO_DATABASE_OVERRIDE"], "true")
        self.assertEqual(self.calls[0]["QUERY"], self.plan["fasta"])
        self.assertNotIn("QUERY", data["request_fields"])
        self.assertNotIn("ACGT", data["url"])
        self.assertIn("RID=ABCD1234567", data["url"])

    def test_program_database_mappings(self):
        for alphabet, database, program in [("nucleotide", "nt", "blastn"), ("nucleotide", "nr", "blastx"),
                                             ("protein", "nr", "blastp"), ("protein", "nt", "tblastn")]:
            plan = dict(self.plan, alphabet=alphabet, database=database, program=program)
            fields = blast.fields_for(plan, self.params)
            self.assertEqual((fields["PROGRAM"], fields["DATABASE"]), (program, database))
            self.assertEqual("NO_DATABASE_OVERRIDE" in fields, database == "nt")
            self.assertEqual(fields["EXPECT"], "0.05")
            self.assertEqual(fields["HITLIST_SIZE"], "100")

    def test_cancel_before_upload_and_cancel_accept_race(self):
        cancel = Path(self.context.cancel_file)
        cancel.touch()
        result = blast.submit_prepared(self.params, self.context, self.transport)
        self.assertEqual(result["data"]["status"], "cancelled")
        self.assertEqual(self.calls, [])
        cancel.unlink()
        def race(fields):
            cancel.touch()
            return self.transport(fields)
        result = blast.submit_prepared(self.params, self.context, race)
        self.assertEqual(result["data"]["status"], "accepted")
        self.assertEqual(len(self.calls), 1)

    def test_preflight_rejects_before_transport(self):
        for change in [{"oversized": True, "truncation": False}, {"query_hash": "changed"}]:
            self.source.write_text(json.dumps(dict(self.plan, **change)))
            with self.assertRaises(ValueError):
                blast.submit_prepared(self.params, self.context, self.transport)
        self.assertEqual(self.calls, [])

    def test_unknown_timeout_and_no_retry(self):
        def timeout(fields):
            self.calls.append(fields)
            raise TimeoutError("response lost")
        result = blast.submit_prepared(self.params, self.context, timeout)
        self.assertEqual(result["data"]["status"], "unknown")
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(result["data"]["rid"], "")

    def test_malformed_and_explicit_service_rejection(self):
        for response in ["<html>oops</html>", "QBlastInfoBegin\nRID = ../evil\nQBlastInfoEnd",
                         "QBlastInfoBegin\nRID = ABCD1234567\nRID = XYZ12345678\nQBlastInfoEnd"]:
            self.assertEqual(blast.parse_response(response)[0], "unknown")
        self.assertEqual(blast.parse_response("Error: Bad query")[0], "rejected")

    def test_receipt_write_failure_still_returns_accepted_data_without_resubmit(self):
        with patch.object(blast, "write_receipt", side_effect=OSError("disk full")):
            result = blast.submit_prepared(self.params, self.context, self.transport)
        self.assertEqual(result["data"]["status"], "accepted")
        self.assertIn("disk full", result["data"]["message"])
        self.assertEqual(len(self.calls), 1)

    def test_restart_receipt_verification_never_sends(self):
        result = blast.submit_prepared(self.params,self.context,self.transport)
        receipt = result["data"]
        recovered = blast.inspect_retained(json.loads((self.root / "receipt").read_text()),self.plan)
        self.assertEqual(recovered,receipt)
        self.assertEqual(len(self.calls),1)
        with self.assertRaises(ValueError):
            blast.inspect_retained(dict(receipt,query_hash="wrong"),self.plan)
        with self.assertRaises(ValueError):
            blast.inspect_retained(dict(receipt,url="https://example.org"),self.plan)

    @unittest.skipIf(os.name == "nt", "Windows uses an owned timeout-bounded child instead of SIGALRM")
    def test_whole_post_deadline_interrupts_hung_read(self):
        with patch.object(blast.http.client,"HTTPSConnection") as connection, patch.object(blast,"POST_SECONDS",0.03):
            response = connection.return_value.getresponse.return_value
            response.status = 200
            response.read.side_effect = lambda size: time.sleep(1)
            started = time.monotonic()
            with self.assertRaises(blast.DeadlineError):
                blast.post({"CMD":"Put"})
            self.assertLess(time.monotonic()-started,0.5)
            self.assertEqual(connection.return_value.request.call_count,1)
            connection.return_value.close.assert_called_once()

    def test_http_wall_deadline_and_response_size_bound(self):
        class Response:
            status = 200
            def read(self, size):
                self.size = size
                return b"x" * size
        response = Response()
        with patch.object(blast.http.client, "HTTPSConnection") as connection:
            connection.return_value.getresponse.return_value = response
            with self.assertRaisesRegex(ValueError, "exceeds"):
                blast._post_request({"CMD": "Put"})
            self.assertEqual(response.size, blast.MAX_RESPONSE + 1)
        with self.assertRaises(blast.DeadlineError):
            blast._deadline(None, None)

    def test_windows_worker_timeout_is_unknown_without_retry(self):
        with patch.object(blast.subprocess, "run", side_effect=subprocess.TimeoutExpired("worker", 30)) as run:
            with self.assertRaises(blast.DeadlineError):
                blast._post_windows({"CMD": "Put", "QUERY": "ACGT"})
            run.assert_called_once()
            self.assertNotIn("ACGT", str(run.call_args.args[0]))
            self.assertEqual(json.loads(run.call_args.kwargs["input"])["QUERY"], "ACGT")
            self.assertEqual(run.call_args.kwargs["timeout"], blast.POST_SECONDS)

    def test_windows_worker_returns_response_and_reports_failure(self):
        with patch.object(blast.subprocess, "run") as run:
            run.return_value = subprocess.CompletedProcess([], 0, "QBlastInfoBegin", "")
            self.assertEqual(blast._post_windows({"CMD": "Put"}), "QBlastInfoBegin")
            run.return_value = subprocess.CompletedProcess([], 1, "", "network failure")
            with self.assertRaisesRegex(ConnectionError, "submission status unknown"):
                blast._post_windows({"CMD": "Put"})

    def test_windows_worker_failure_retains_unknown_receipt_without_retry(self):
        failed = subprocess.CompletedProcess([], 1, "", "connection reset after upload")
        with patch.object(blast.subprocess, "run", return_value=failed) as run:
            result = blast.submit_prepared(self.params, self.context, blast._post_windows)
            run.assert_called_once()
        receipt = json.loads((self.root / "receipt").read_text())
        self.assertEqual(receipt, result["data"])
        self.assertEqual(receipt["status"], "unknown")
        self.assertEqual((receipt["rid"], receipt["url"]), ("", ""))
        self.assertIn("Do not automatically resubmit", receipt["message"])
        self.assertIn("connection reset after upload", receipt["message"])
        self.assertEqual(result["artifacts"], [{"role": "receipt", "path": "."}])
        self.assertEqual(blast.inspect_retained(receipt, self.plan), receipt)


if __name__ == "__main__":
    unittest.main()
