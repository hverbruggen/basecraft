#!/usr/bin/env python3
"""Measure a native executable; never computes PL or changes its input."""

import hashlib
import json
from pathlib import Path
import resource
import subprocess
import sys
import time


binary, request, output = map(Path, sys.argv[1:4])
started = time.monotonic()
with request.open("rb") as source, output.open("wb") as destination:
    process = subprocess.run([str(binary)], stdin=source, stdout=destination,
                             stderr=subprocess.PIPE, check=False)
elapsed = time.monotonic() - started
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
print(json.dumps({
    "request": str(request),
    "request_sha256": hashlib.sha256(request.read_bytes()).hexdigest(),
    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    "exit_code": process.returncode,
    "elapsed_seconds": elapsed,
    "child_user_seconds": usage.ru_utime,
    "child_system_seconds": usage.ru_stime,
    "peak_rss_kib": usage.ru_maxrss,
    "stderr": process.stderr.decode("utf-8", errors="replace")[-2000:],
}, sort_keys=True))
if process.returncode:
    sys.exit(process.returncode)
