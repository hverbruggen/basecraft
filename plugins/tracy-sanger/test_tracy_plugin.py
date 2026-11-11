"""Executed caller/input identity must remain stable around the public operation."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from basecraft.plugin import local_context
from consensus import file_hash
from tracy_plugin import consensus


class ExecutedIdentityTests(unittest.TestCase):
    def test_changed_executing_caller_is_not_accepted(self):
        with tempfile.TemporaryDirectory() as folder:
            caller = Path(folder) / "caller"
            caller.write_bytes(b"original engine")
            source = Path(folder) / "read.ab1"
            source.write_bytes(b"ABIF original input")
            engine_hash = file_hash(caller)

            def changed(*args, **kwargs):
                caller.write_bytes(b"replaced during execution")
                return {"engine_sha256": engine_hash, "inputs": [{"sha256": file_hash(source)}]}

            with (
                patch("tracy_plugin.executable", return_value=str(caller)),
                patch("tracy_plugin.merge", side_effect=changed),
                self.assertRaisesRegex(ValueError, "Tracy changed during"),
            ):
                consensus({"engine_sha256": engine_hash},
                          local_context(folder, {"reads": [str(source)]}, {"artifacts": "output"}))

    def test_changed_input_is_not_accepted(self):
        with tempfile.TemporaryDirectory() as folder:
            caller = Path(folder) / "caller"
            caller.write_bytes(b"original engine")
            source = Path(folder) / "read.ab1"
            source.write_bytes(b"ABIF original input")
            source_hash = file_hash(source)

            def changed(*args, **kwargs):
                source.write_bytes(b"ABIF replaced during execution")
                return {"engine_sha256": file_hash(caller), "inputs": [{"sha256": source_hash}]}

            with (
                patch("tracy_plugin.executable", return_value=str(caller)),
                patch("tracy_plugin.merge", side_effect=changed),
                self.assertRaisesRegex(ValueError, "input changed during"),
            ):
                consensus({}, local_context(folder, {"reads": [str(source)]}, {"artifacts": "output"}))
