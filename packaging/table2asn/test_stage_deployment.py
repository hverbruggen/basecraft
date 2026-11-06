"""Deployment metadata must not change qualified code or package selections."""

import hashlib
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from stage_deployment import stage


class StagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.source = root / "source"
        self.private = root / "private"
        self.plugin = root / "candidate"
        for path in (self.source, self.private, self.plugin):
            path.mkdir()
        self.channel = root / "channel"
        (self.channel / "linux-64").mkdir(parents=True)
        self.package = self.channel / "linux-64/tool.conda"
        self.package.write_bytes(b"qualified-package")
        manifest = '''schema = 2
id = "basecraft.table2asn"
[entry]
operation = "python:table2asn_plugin.py"
[runtime]
platforms = ["linux-64"]
channels = ["conda-forge"]
executables = ["table2asn"]
[runtime.dependencies]
basecraft-table2asn = "==30.8.0"
'''
        (self.source / "plugin.toml").write_text(manifest)
        shutil.copy2(self.source / "plugin.toml", self.plugin / "plugin.toml")
        qualified = manifest.replace('channels = ["conda-forge"]',
                                     f'lock = "pixi.lock"\nchannels = ["{self.channel.as_uri()}", "conda-forge"]')
        (self.private / "plugin.toml").write_text(qualified)
        (self.private / "pixi.lock").write_bytes(b"qualified-lock")
        self.artifact = root / "artifact.json"
        self.artifact.write_text(json.dumps({
            "target": "linux-64",
            "qualified_deployment_lock_sha256": hashlib.sha256(b"qualified-lock").hexdigest(),
            "package": {"filename": "tool.conda", "bytes": 17,
                        "sha256": hashlib.sha256(b"qualified-package").hexdigest()}
        }))

    def stage(self):
        stage(self.private, self.plugin, self.source, self.artifact)

    def test_qualified_package_stages_idempotently_and_preserves_source(self):
        original = (self.source / "plugin.toml").read_bytes()
        self.stage()
        self.stage()
        self.assertEqual((self.plugin / "pixi.lock").read_bytes(), b"qualified-lock")
        self.assertEqual((self.source / "plugin.toml").read_bytes(), original)
        self.assertFalse((self.source / "pixi.lock").exists())
        with self.assertRaisesRegex(ValueError, "never the source"):
            stage(self.private, self.source, self.source, self.artifact)

    def test_changed_lock_package_or_requirements_leave_candidate_unchanged(self):
        original = (self.plugin / "plugin.toml").read_bytes()
        for path, data in [
            (self.private / "pixi.lock", b"changed lock"),
            (self.package, b"changed package"),
            (self.private / "plugin.toml",
             (self.private / "plugin.toml").read_bytes().replace(b"==30.8.0", b"*")),
            (self.private / "plugin.toml",
             (self.private / "plugin.toml").read_bytes().replace(b"basecraft.table2asn", b"other.plugin")),
        ]:
            with self.subTest(path=path, data=data):
                before = path.read_bytes()
                try:
                    path.write_bytes(data)
                    with self.assertRaises(ValueError):
                        self.stage()
                    self.assertEqual((self.plugin / "plugin.toml").read_bytes(), original)
                    self.assertFalse((self.plugin / "pixi.lock").exists())
                finally:
                    path.write_bytes(before)


if __name__ == "__main__":
    unittest.main()
