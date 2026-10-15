"""Regression checks for launch without compilation and atomic publication."""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import test_build
from test_build import switch_link

LAUNCHER = Path(__file__).with_name("basecraft-launch.py")


class DeliveryTests(unittest.TestCase):
    def test_snapshot_excludes_unrelated_private_files_and_tracks_changed_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates").mkdir()
            (root / "crates/lib.rs").write_text("first")
            (root / ".env").write_text("private test value")
            with (
                patch.object(test_build, "REPO", root),
                patch.object(
                    test_build.subprocess,
                    "check_output",
                    return_value=b"crates/lib.rs\0.env\0",
                ),
            ):
                one = root / "one"
                one.mkdir()
                _, source, first = test_build.snapshot(one)
                self.assertFalse((source / ".env").exists())
                self.assertEqual((source / "crates/lib.rs").read_text(), "first")
                (root / "crates/lib.rs").write_text("other")
                two = root / "two"
                two.mkdir()
                _, _, second = test_build.snapshot(two)
                self.assertNotEqual(first, second)

    def test_launcher_uses_published_bundle_and_preserves_arguments_and_cwd(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = root / "release"
            (release / "bin").mkdir(parents=True)
            fake = "#!/usr/bin/python3\nimport os,sys,json\nprint(json.dumps([sys.argv,os.getcwd(),os.environ['BASECRAFT_PLUGINS']]))\n"
            for name in ("basecraft", "basecraft-gui"):
                path = release / "bin" / name
                path.write_text(fake)
                path.chmod(0o755)
            (release / "build.json").write_text(
                json.dumps({"python_environment": str(root / "absent")})
            )
            switch_link(root / "current", release)
            branch = "feature/space name"
            switch_link(
                root / "channels" / hashlib.sha256(branch.encode()).hexdigest()[:20],
                release,
            )
            env = dict(os.environ, BASECRAFT_BUILDS=str(root), PATH="/no-compiler-here")
            env.pop("BASECRAFT_PLUGINS", None)
            for args, executable in [
                (
                    ["--", "relative file 藻.afa", "--screenshot", "output.png"],
                    "basecraft-gui",
                ),
                (["--cli", "history", "file.fa"], "basecraft"),
                (["--branch", branch, "--cli", "--help"], "basecraft"),
            ]:
                result = subprocess.run(
                    [sys.executable, str(LAUNCHER), *args],
                    env=env,
                    cwd=root,
                    capture_output=True,
                    text=True,
                    check=True,
                )
                argv, cwd, plugins = json.loads(result.stdout)
                self.assertEqual(Path(argv[0]).name, executable)
                self.assertEqual(cwd, str(root))
                self.assertEqual(plugins, str(release / "plugins"))
                if args[0] == "--":
                    self.assertEqual(argv[1:], args[1:])

    def test_missing_build_fails_without_compiling(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, str(LAUNCHER), "--cli", "--help"],
                env=dict(os.environ, BASECRAFT_BUILDS=directory, PATH="/absent"),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("No published test build", result.stderr)

    def test_switch_preserves_complete_old_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            old, new = root / "old", root / "new"
            old.mkdir()
            new.mkdir()
            (old / "evidence").write_text("keep")
            switch_link(root / "current", old)
            switch_link(root / "current", new)
            self.assertEqual((root / "current").resolve(), new)
            self.assertEqual((old / "evidence").read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
