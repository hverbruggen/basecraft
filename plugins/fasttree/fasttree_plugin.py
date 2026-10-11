"""Infer a maximum-likelihood tree with FastTree's standard settings.

The host passes an aligned FASTA file and the path to write the tree to.
DNA and RNA get the GTR model (`-nt -gtr`), proteins LG (`-lg`), both with
FastTree's default CAT rate categories and SH-like local support values.
Sequences go to FastTree under neutral ids and get their names back in the
tree, so names with spaces or Newick's reserved characters survive.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin

plugin = Plugin("basecraft.fasttree", "0.1.0")

NUCLEOTIDES = set("ACGTUNRYKMSWBDHV")


def fasttree_executable() -> str:
    """$BASECRAFT_FASTTREE, else FastTree next to this Python, else the PATH."""
    if path := os.environ.get("BASECRAFT_FASTTREE"):
        return path
    for name in ("FastTree", "fasttree"):
        beside = Path(sys.prefix) / "bin" / name
        if beside.exists():
            return str(beside)
    for name in ("FastTree", "fasttree"):
        if found := shutil.which(name):
            return found
    raise FileNotFoundError("FastTree isn't installed: run `pixi install`, or set BASECRAFT_FASTTREE")


def fasttree_version(exe: str) -> str:
    out = subprocess.run([exe, "-expert"], capture_output=True, text=True, check=False)
    # "Detailed usage for FastTree 2.2.0 Double precision:"
    match = re.search(r"FastTree (\d[\w.]*)", out.stderr + out.stdout)
    return match.group(1) if match else "unknown"


def read_fasta(path: Path) -> list[tuple[str, str]]:
    """(name, sequence) pairs; the name is the header's first word."""
    records: list[tuple[str, list[str]]] = []
    with path.open() as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith(">"):
                header = line[1:].strip()
                records.append((header.split()[0] if header else f"seq{len(records) + 1}", []))
            elif records:
                records[-1][1].append(line.strip())
    return [(name, "".join(parts)) for name, parts in records]


def is_nucleotide(records: list[tuple[str, str]]) -> bool:
    letters = [c for _, seq in records for c in seq.upper() if c.isalpha()]
    if not letters:
        return True
    return sum(c in NUCLEOTIDES for c in letters) / len(letters) > 0.9


def quote(name: str) -> str:
    """A name as Newick writes it: quoted when it holds reserved characters."""
    if name and not re.search(r"[()\[\]',:;=\s]", name):
        return name
    return "'" + name.replace("'", "''") + "'"


@plugin.operation("tree")
def tree(snapshot, params):
    """Infer a tree from the alignment in `input` and write it to `output`."""
    source = Path(params["input"])
    target = Path(params["output"])
    records = read_fasta(source)
    if len(records) < 3:
        raise ValueError("FastTree needs at least three sequences")
    nucleotide = is_nucleotide(records)
    exe = fasttree_executable()
    ids = [f"t{i}" for i in range(len(records))]
    with tempfile.TemporaryDirectory(prefix="basecraft-fasttree") as work:
        renamed = Path(work) / "input.fasta"
        with renamed.open("w") as f:
            for id_, (_, seq) in zip(ids, records):
                f.write(f">{id_}\n{seq}\n")
        args = [exe, "-quiet"] + (["-nt", "-gtr"] if nucleotide else ["-lg"]) + [str(renamed)]
        run = subprocess.run(args, capture_output=True, text=True, check=False)
    newick = run.stdout.strip()
    if run.returncode != 0 or not newick.endswith(";"):
        detail = run.stderr.strip().splitlines()
        raise RuntimeError(f"FastTree failed: {detail[-1] if detail else run.returncode}")
    names = {id_: quote(name) for id_, (name, _) in zip(ids, records)}
    newick = re.sub(r"(?<=[(,])(t\d+)(?=[:,);])", lambda m: names[m.group(1)], newick)
    partial = target.with_name(target.name + ".part")
    partial.write_text(newick + "\n")
    os.replace(partial, target)
    model = "GTR" if nucleotide else "LG"
    return {
        "ops": [],
        "message": f"Built a {model} tree of {len(records)} sequences",
        "outputs": [str(target)],
        "tool": {"name": "fasttree", "version": fasttree_version(exe)},
    }


if __name__ == "__main__":
    plugin.run()
