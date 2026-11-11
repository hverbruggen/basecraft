"""Utilities for public plugins that invoke local or pixi-installed tools."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def allocated_cpus(requested=None):
    """Resolve a tool's worker count within the host's local CPU allocation."""
    if requested is not None and (
        isinstance(requested, bool) or not isinstance(requested, int) or requested < 1
    ):
        raise ValueError("CPU threads must be a positive integer")
    available = os.cpu_count() or 1
    if "BASECRAFT_CPU_THREADS" in os.environ:
        budget = int(os.environ["BASECRAFT_CPU_THREADS"])
    else:
        # Use the Rust reader for paths, schema/migration and validation. Never
        # silently reinterpret a malformed profile or use a divergent TOML parser.
        try:
            process = subprocess.run(
                [executable(["basecraft"], "BASECRAFT"), "preferences"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=True,
                timeout=15,
            )
        except subprocess.CalledProcessError as error:
            raise ValueError(f"Cannot load Basecraft preferences: {(error.stderr or str(error)).strip()}") from error
        except subprocess.TimeoutExpired as error:
            raise ValueError("Timed out loading Basecraft preferences") from error
        except FileNotFoundError as error:
            raise ValueError("Install the Basecraft CLI or set BASECRAFT to load shared preferences") from error
        try:
            budget = json.loads(process.stdout)["compute"]["local_cpu_threads"]
        except (ValueError, KeyError, TypeError) as error:
            raise ValueError("Basecraft returned an invalid preferences snapshot") from error
        if isinstance(budget, bool) or not isinstance(budget, int) or not 1 <= budget <= 65536:
            raise ValueError("Basecraft returned an invalid CPU allocation")
    if not 1 <= budget <= 65536:
        raise ValueError("CPU allocation must be between 1 and 65536")
    budget = min(budget, available)
    if requested is None:
        return budget
    return min(requested, budget)


def executable(names, environment):
    if override := os.environ.get(environment):
        return override
    for name in names:
        beside = Path(sys.prefix) / "bin" / name
        if beside.is_file() and os.access(beside, os.X_OK):
            return str(beside)
        if found := shutil.which(name):
            return found
    raise FileNotFoundError(f"Install {names[0]} with `pixi install`, or set {environment}")


def read_alignment(path, minimum=2):
    records = []
    with Path(path).open() as source:
        for line in source:
            line = line.strip()
            if not line:
                continue
            if line.startswith(">"):
                header = line[1:].strip()
                if not header:
                    raise ValueError("Empty FASTA identifier")
                records.append((header, []))
            elif records:
                records[-1][1].append("".join(line.split()))
            else:
                raise ValueError("Sequence before FASTA header")
    records = [(h, "".join(s)) for h, s in records]
    names = [h.split()[0] for h, _ in records]
    if len(set(names)) != len(names):
        raise ValueError("Sequence identifiers must be unique")
    if len(records) < minimum or not records[0][1]:
        raise ValueError(f"At least {minimum} nonempty aligned sequences are required")
    if len({len(s) for _, s in records}) != 1:
        raise ValueError("Sequences must have equal aligned lengths")
    return records


def write_atomic(path, text):
    path = Path(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as out:
            temporary = Path(out.name)
            out.write(text)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def quote_newick(name):
    return "'" + name.replace("'", "''") + "'"
