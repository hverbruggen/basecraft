#!/usr/bin/env python3
"""Install the upstream ASAP engine (iTaxoTools ASAPy) for the delimitation plugin.

ASAPy is published only as C source on GitHub, so it is built into its own
virtual environment instead of the locked project environment. The launcher
points BASECRAFT_ASAP_PYTHON at it when present. Its Qt GUI dependency is
skipped; only the C extension and its small parameter helper are installed.

    python3 tools/install-asap.py [--python PATH] [--prefix DIR]
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

COMMIT = "7cd38ba422513ee4ab0f4b4672a661edee89dd45"
SOURCE = f"git+https://github.com/iTaxoTools/ASAPy@{COMMIT}"
# Upstream pins 0.2.4, which is not on PyPI; 0.3.5 provides the same `param` module.
COMMON = "itaxotools-common==0.3.5"
# The 2023 C sources predate GCC 14, which made these diagnostics errors. The
# undeclared calls return void or int, so the implicit declarations are benign;
# the method-table cast is CPython's usual METH_KEYWORDS idiom.
CFLAGS = " ".join(f"-Wno-error={flag}" for flag in
                  ("implicit-function-declaration", "incompatible-pointer-types", "int-conversion"))
DEFAULT_PREFIX = Path.home() / ".cache/basecraft/tools/asap"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--python", default=sys.executable,
                        help="interpreter to base the environment on (default: this one)")
    parser.add_argument("--prefix", type=Path, default=DEFAULT_PREFIX)
    args = parser.parse_args()
    if shutil.which("cc") is None and shutil.which("gcc") is None:
        sys.exit("A C compiler is required to build ASAP")
    if args.prefix.exists():
        shutil.rmtree(args.prefix)
    subprocess.run([args.python, "-m", "venv", str(args.prefix)], check=True)
    python = args.prefix / "bin/python"
    env = dict(os.environ, CFLAGS=(os.environ.get("CFLAGS", "") + " " + CFLAGS).strip())
    subprocess.run([python, "-m", "pip", "install", "--quiet", "--no-deps", SOURCE, COMMON], check=True, env=env)
    subprocess.run([python, "-c", "import itaxotools.asapy.asap"], check=True)
    print(f"ASAP {COMMIT[:7]} installed: {python}")


if __name__ == "__main__":
    main()
