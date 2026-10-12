"""Utilities for public plugins that invoke local or pixi-installed tools."""

import os
import shutil
import sys
import tempfile
from pathlib import Path


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
