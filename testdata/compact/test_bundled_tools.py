"""Pinned downloads must be verified before any executable is staged."""
import hashlib
import importlib.util
import io
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("bundled_tools", ROOT / "tools/bundled_tools.py")
TOOLS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TOOLS)


class BundledTools(unittest.TestCase):
    def test_bad_download_never_enters_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            with (
                patch.object(TOOLS.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong")),
                self.assertRaisesRegex(RuntimeError, "SHA-256"),
            ):
                TOOLS.cached_asset(cache, "https://example.invalid/tool", "a" * 64)
            self.assertEqual(list(cache.iterdir()), [])

    def test_verified_cache_is_reused_but_corruption_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            digest = hashlib.sha256(b"tool").hexdigest()
            target = cache / digest
            target.write_bytes(b"tool")
            with patch.object(TOOLS.urllib.request, "urlopen") as network:
                self.assertEqual(TOOLS.cached_asset(cache, "https://example.invalid/tool", digest), target)
                network.assert_not_called()
                target.write_bytes(b"changed")
                with self.assertRaisesRegex(RuntimeError, "SHA-256"):
                    TOOLS.cached_asset(cache, "https://example.invalid/tool", digest)
                network.assert_not_called()

    def test_archive_links_cannot_supply_executables(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "tool.tar"
            with tarfile.open(archive, "w") as stream:
                member = tarfile.TarInfo("bin/tool")
                member.type = tarfile.SYMTYPE
                member.linkname = "/outside/tool"
                stream.addfile(member)
            destination = Path(directory) / "output"
            with (
                tarfile.open(archive) as stream,
                self.assertRaisesRegex(RuntimeError, "Invalid tool archive"),
            ):
                TOOLS.copy_member(stream, "bin/tool", destination)
            self.assertFalse(destination.exists())
