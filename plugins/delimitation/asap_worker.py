"""Isolated upstream ASAP extension: its C code may call exit()."""
import hashlib
import importlib
import importlib.metadata
import json
import sys
from pathlib import Path

request = json.loads(Path(sys.argv[1]).read_text())
metadata = Path(request.pop("worker_metadata"))
try:
    asap = importlib.import_module("itaxotools.asapy.asap")
except ModuleNotFoundError:
    asap = importlib.import_module("asap")
try:
    version = importlib.metadata.version("asapy")
except importlib.metadata.PackageNotFoundError:
    version = "unpackaged ASAP extension"
module = Path(asap.__file__)
metadata.write_text(json.dumps({"version": version, "module": str(module),
                                "module_sha256": hashlib.sha256(module.read_bytes()).hexdigest()})+"\n")
asap.main(request.pop("input"), **request)
