"""Isolated upstream ASAP extension: its C code may call exit()."""
import importlib
import json
import sys
from pathlib import Path

request = json.loads(Path(sys.argv[1]).read_text())
try:
    asap = importlib.import_module("itaxotools.asapy.asap")
except ModuleNotFoundError:
    asap = importlib.import_module("asap")
asap.main(request.pop("input"), **request)
