"""Utilities for public plugins that invoke local or pixi-installed tools."""

import json
import os
import re
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


def bootstrap_collection_path(params):
    """Where to write a run's bootstrap trees: the host-allocated metadata or
    working directory chosen by `bootstrap_destination`, or an explicit
    absolute `bootstrap_trees` path. Never creates or guesses a namespace."""
    destination = params.get("bootstrap_destination", "metadata")
    if destination not in ("metadata", "working"):
        raise ValueError("Bootstrap destination must be metadata or working")
    if "bootstrap_trees" in params:
        if "analysis_artifacts" in params:
            raise ValueError("bootstrap_trees and analysis_artifacts are ambiguous together")
        path = Path(params["bootstrap_trees"])
        if not path.is_absolute() or path.is_symlink():
            raise ValueError("bootstrap_trees must be an absolute, non-symlink output path")
        return path.parent.resolve(strict=True) / path.name
    allocation = params.get("analysis_artifacts")
    if not isinstance(allocation, dict):
        raise TypeError("Bootstrap retention requires host analysis_artifacts allocation or an explicit absolute bootstrap_trees path")
    run_id = allocation.get("run_id")
    if not isinstance(run_id, str) or not re.fullmatch(r"[A-Za-z0-9-]+", run_id):
        raise ValueError("Invalid bootstrap artifact run_id")
    key = "metadata_directory" if destination == "metadata" else "working_directory"
    value = allocation.get(key)
    if not isinstance(value, str):
        raise TypeError(f"Missing allocated {key}")
    directory = Path(value)
    if not directory.is_absolute() or not directory.is_dir() or directory.resolve() != directory:
        raise ValueError(f"Allocated {key} must be an existing absolute directory without symlinks")
    return directory / f"bootstrap-{run_id}.nex"
