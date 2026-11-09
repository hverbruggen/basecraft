"""Contract fixtures based on https://developers.zenodo.org/ (deposit + bucket API)."""
import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import zenodo_plugin as z


class Zenodo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.file = self.root / "a file.fa"
        self.file.write_bytes(b">synthetic\nACGT\n")
        self.context = {"inputs": {"files": [str(self.file)]}, "input_evidence": {"files": [{"original_path": str(self.file)}]},
                        "outputs": {"receipt": str(self.root / "selection.json")},
                        "cancel_file": str(self.root / "cancel"), "progress_file": str(self.root / "progress")}
        self.plan = z.prepare(self.context)["data"]
        self.context["inputs"]["selection"] = [self.context["outputs"]["receipt"]]
        self.context["outputs"]["receipt"] = str(self.root / "draft.json")
        self.parameters = {"title": "Synthetic dataset", "description": "Transport fixture only", "creators": "Example, A.",
                           "license": "cc-by-4.0", "service": "sandbox", "access_right": "open"}
        self.calls = []
        self.remote = {"id": 123, "submitted": False, "metadata": z.metadata(self.parameters),
                       "links": {"bucket": "https://sandbox.zenodo.org/api/files/test-bucket"}, "files": []}

    def transport(self, method, url, token, context, **kwargs):
        self.calls.append((method, url, kwargs))
        self.assertEqual(token, "private-test-token")
        if method == "POST":
            self.assertEqual(url, "https://sandbox.zenodo.org/api/deposit/depositions")
            self.assertEqual(kwargs["body"], {"metadata": z.metadata(self.parameters)})
            return copy.deepcopy(self.remote)
        if method == "GET":
            self.assertEqual(url, "https://sandbox.zenodo.org/api/deposit/depositions/123")
            return copy.deepcopy(self.remote)
        self.assertEqual(method, "PUT")
        self.assertTrue(url.endswith("/a%20file.fa"))
        content = Path(kwargs["file"]).read_bytes()
        self.assertEqual(kwargs["size"], len(content))
        checksum = hashlib.md5(content, usedforsecurity=False).hexdigest()
        self.remote["files"].append({"filename": "a file.fa", "filesize": len(content), "checksum": checksum})
        return {"key": "a file.fa", "size": len(content), "checksum": "md5:" + checksum}

    def upload(self, transport=None):
        return z.upload(self.parameters, self.context, transport or self.transport, "private-test-token")["data"]

    def resume(self):
        self.context["inputs"]["resume"] = [self.context["outputs"]["receipt"]]
        # Runner freezes resume input before allocating next output.
        previous = self.root / "previous.json"
        previous.write_bytes(Path(self.context["outputs"]["receipt"]).read_bytes())
        self.context["inputs"]["resume"] = [str(previous)]

    def test_documented_create_and_raw_object_put_never_publish(self):
        data = self.upload()
        self.assertEqual(data["status"], "draft_ready")
        self.assertEqual([call[0] for call in self.calls], ["POST", "PUT"])
        self.assertEqual(data["draft_id"], 123)
        self.assertNotIn("doi", data)
        self.assertNotIn("private-test-token", Path(self.context["outputs"]["receipt"]).read_text())
        self.assertNotIn("actions/publish", json.dumps(self.calls))

    def test_retry_checks_same_draft_and_skips_verified_uploaded_bytes(self):
        self.upload()
        self.resume()
        self.calls.clear()
        self.assertEqual(self.upload()["status"], "draft_ready")
        self.assertEqual([c[0] for c in self.calls], ["GET"])

    def test_partial_upload_retries_without_duplicate_deposit(self):
        def failed(method, *args, **kwargs):
            if method == "PUT":
                raise OSError("private-test-token must never escape")
            return self.transport(method, *args, **kwargs)
        partial = self.upload(failed)
        self.assertEqual(partial["status"], "upload_unknown")
        self.assertEqual(partial["draft_id"], 123)
        self.resume()
        self.calls.clear()
        self.assertEqual(self.upload()["status"], "draft_ready")
        self.assertEqual([c[0] for c in self.calls], ["GET", "PUT"])

    def test_uncertain_create_is_not_retried(self):
        def failed(*args, **kwargs):
            raise OSError("private-test-token must never escape")
        receipt = self.upload(failed)
        self.assertEqual(receipt["status"], "creation_unknown")
        self.assertNotIn("private-test-token", json.dumps(receipt))
        self.resume()
        with self.assertRaisesRegex(ValueError, "Creation status unknown"):
            self.upload()
        self.assertEqual(self.calls, [])

    def test_selection_change_fails_before_network(self):
        self.file.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "bytes changed"):
            self.upload()
        self.assertEqual(self.calls, [])

    def test_retry_changed_metadata_fails_before_network(self):
        self.upload()
        self.resume()
        self.calls.clear()
        self.parameters["title"] = "Another dataset"
        with self.assertRaisesRegex(ValueError, "exact previous"):
            self.upload()
        self.assertEqual(self.calls, [])

    def test_no_automatic_remote_overwrite(self):
        self.upload()
        self.resume()
        self.remote["files"][0]["checksum"] = "wrong"
        self.calls.clear()
        receipt = self.upload()
        self.assertIn("overwrite refused", receipt["message"])
        self.assertEqual([c[0] for c in self.calls], ["GET"])

    def test_remote_metadata_change_blocks_upload(self):
        self.remote["metadata"]["title"] = "Changed on Zenodo"
        receipt = self.upload()
        self.assertEqual(receipt["draft_id"], 123)
        self.assertIn("metadata changed", receipt["message"])
        self.assertEqual([c[0] for c in self.calls], ["POST"])

    def test_credential_never_sent_to_redirect_or_other_origin(self):
        self.remote["links"]["bucket"] = "https://example.org/api/files/leak"
        receipt = self.upload()
        self.assertIn("unexpected URL", receipt["message"])
        self.assertEqual([c[0] for c in self.calls], ["POST"])

    def test_cancellation_before_and_after_creation(self):
        Path(self.context["cancel_file"]).touch()
        with self.assertRaises(z.Cancelled):
            self.upload()
        self.assertEqual(self.calls, [])
        Path(self.context["cancel_file"]).unlink()
        def cancel_after_create(*args, **kwargs):
            response = self.transport(*args, **kwargs)
            Path(self.context["cancel_file"]).touch()
            return response
        receipt = self.upload(cancel_after_create)
        self.assertEqual(receipt["draft_id"], 123)
        self.assertEqual(receipt["status"], "cancelled")
        self.assertEqual([c[0] for c in self.calls], ["POST"])

    def test_metadata_validation_and_closed_access(self):
        for key in ("title", "description", "creators", "license"):
            with self.assertRaises(ValueError):
                z.metadata({**self.parameters, key: " "})
        self.parameters["access_right"] = "closed"
        self.assertNotIn("license", z.metadata(self.parameters))
        with self.assertRaises(ValueError):
            z.metadata({**self.parameters, "access_right": "restricted"})

    def test_duplicate_filename_rejected(self):
        self.context["inputs"]["files"] *= 2
        self.context["input_evidence"]["files"] *= 2
        with self.assertRaisesRegex(ValueError, "distinct"):
            z.prepare(self.context)

    def test_transport_headers_raw_bytes_and_redaction(self):
        class Response:
            status = 200
            def read(self, limit):
                return b'{"key":"a file.fa"}'
        class Connection:
            def __init__(self, host, timeout):
                self.host = host
                self.headers = {}
                self.sent = bytearray()
            def putrequest(self, method, path): self.method, self.path = method, path
            def putheader(self, key, value): self.headers[key] = value
            def endheaders(self): pass
            def send(self, data): self.sent.extend(data)
            def getresponse(self): return Response()
            def close(self): pass
        connection = Connection("", 30)
        with patch.object(z.http.client, "HTTPSConnection", return_value=connection):
            z.request("PUT", "https://sandbox.zenodo.org/api/files/bucket/a%20file.fa", "private-test-token", self.context,
                      file=str(self.file), size=self.file.stat().st_size)
        self.assertEqual(bytes(connection.sent), self.file.read_bytes())
        self.assertEqual(connection.headers["Authorization"], "Bearer private-test-token")
        self.assertEqual(connection.headers["Content-Type"], "application/octet-stream")
        self.assertNotIn("token", connection.path)
        self.assertNotIn("multipart", json.dumps(connection.headers))

    def test_authentication_failure_and_redirect_never_followed(self):
        class Response:
            status = 401
            def read(self, limit): return b'private-test-token'
        from unittest.mock import MagicMock
        connection = MagicMock()
        connection.getresponse.return_value = Response()
        with patch.object(z.http.client, "HTTPSConnection", return_value=connection):
            receipt = self.upload(z.request)
        self.assertIn("Authentication expired", receipt["message"])
        self.assertNotIn("private-test-token", json.dumps(receipt))
        connection.getresponse.return_value.status = 302
        with patch.object(z.http.client, "HTTPSConnection", return_value=connection):
            with self.assertRaisesRegex(z.RemoteError, "HTTP 302"):
                z.request("GET", "https://sandbox.zenodo.org/api/deposit/depositions/1", "private-test-token", self.context)


if __name__ == "__main__":
    unittest.main()
