"""Shared resource limits used by local tool wrappers."""

import json
import os
import subprocess
import unittest
from unittest.mock import patch

from basecraft.tools import allocated_cpus


class CpuAllocationTests(unittest.TestCase):
    def test_defaults_and_explicit_worker_counts_respect_the_allocation(self):
        with (
            patch("basecraft.tools.os.cpu_count", return_value=8),
            patch.dict(os.environ, {"BASECRAFT_CPU_THREADS": "2"}),
        ):
            self.assertEqual(allocated_cpus(), 2)
            self.assertEqual(allocated_cpus(8), 2)
            self.assertEqual(allocated_cpus(1), 1)
            for invalid in (0, -1, True, "8"):
                with self.assertRaises(ValueError):
                    allocated_cpus(invalid)

    def test_standalone_uses_the_validated_rust_preferences_reader(self):
        with (
            patch.dict(os.environ, {}, clear=True),
            patch("basecraft.tools.os.cpu_count", return_value=8),
            patch("basecraft.tools.executable", return_value="/test/basecraft"),
            patch("basecraft.tools.subprocess.run") as run,
        ):
            run.return_value.stdout = json.dumps({"compute": {"local_cpu_threads": 3}})
            self.assertEqual(allocated_cpus(), 3)
            self.assertEqual(run.call_args.args[0], ["/test/basecraft", "preferences"])
            for invalid in (True, 0, 65537, "3"):
                run.return_value.stdout = json.dumps({"compute": {"local_cpu_threads": invalid}})
                with self.assertRaises(ValueError):
                    allocated_cpus()
            run.side_effect = subprocess.CalledProcessError(1, ["basecraft"], stderr="invalid profile")
            with self.assertRaisesRegex(ValueError, "invalid profile"):
                allocated_cpus()

    def test_host_allocation_does_not_require_the_cli(self):
        with (
            patch.dict(os.environ, {"BASECRAFT_CPU_THREADS": "2"}),
            patch("basecraft.tools.subprocess.run") as run,
        ):
            self.assertGreaterEqual(allocated_cpus(), 1)
            run.assert_not_called()
