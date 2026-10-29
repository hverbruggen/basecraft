"""Pinned host selections must never fall back to a different ambient version."""
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("table2asn_plugin", Path(__file__).with_name("table2asn_plugin.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RuntimeSelection(unittest.TestCase):
    def test_selected_runtime_wins_and_missing_or_invalid_selection_fails_closed(self):
        with tempfile.TemporaryDirectory() as folder:
            pinned = Path(folder) / "pinned"
            ambient = Path(folder) / "ambient"
            pinned.touch()
            ambient.touch()
            environment = {"BASECRAFT_TABLE2ASN": str(ambient),
                           "BASECRAFT_RUNTIME_EXECUTABLES": json.dumps({"table2asn": str(pinned)})}
            with patch.dict(os.environ, environment, clear=True):
                self.assertEqual(module.executable(), str(pinned))
                pinned.unlink()
                self.assertIsNone(module.executable())
                for bad in ("{", "[]", "null", "{}", '{"table2asn": 4}', '{"table2asn": "relative"}'):
                    os.environ["BASECRAFT_RUNTIME_EXECUTABLES"] = bad
                    self.assertIsNone(module.executable(), bad)
                del os.environ["BASECRAFT_RUNTIME_EXECUTABLES"]
                self.assertEqual(module.executable(), str(ambient))


if __name__ == "__main__":
    unittest.main()
