"""Align sequences with MAFFT, using its default strategy (FFT-NS-2).

The host passes an unaligned FASTA file and the path to write the
alignment to. Residue case is kept as it was in the input.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import allocated_cpus

plugin = Plugin("basecraft.mafft", "0.2.0")


def mafft_executable() -> str:
    """$BASECRAFT_MAFFT, else MAFFT next to this Python, else the PATH."""
    if path := os.environ.get("BASECRAFT_MAFFT"):
        return path
    beside = Path(sys.prefix) / "bin" / "mafft"
    if beside.exists():
        return str(beside)
    if found := shutil.which("mafft"):
        return found
    raise FileNotFoundError("MAFFT isn't installed: run `pixi install`, or set BASECRAFT_MAFFT")


def mafft_version(exe: str) -> str:
    out = subprocess.run([exe, "--version"], capture_output=True, text=True, check=False)
    # MAFFT prints e.g. "v7.526 (2024/Apr/26)" to stderr.
    text = (out.stderr or out.stdout).strip()
    return text.split()[0] if text else "unknown"


@plugin.operation("align")
def align(snapshot, params):
    """Align the sequences in `input` and write the alignment to `output`."""
    source = Path(params["input"])
    target = Path(params["output"])
    count = sum(1 for line in source.open("rb") if line.startswith(b">"))
    if count < 2:
        raise ValueError("MAFFT needs at least two sequences")
    exe = mafft_executable()
    partial = target.with_name(target.name + ".part")
    threads = allocated_cpus(params.get("threads"))
    argv = [exe, "--preservecase", "--thread", str(threads), str(source)]
    with partial.open("wb") as out:
        run = subprocess.run(argv, stdout=out, stderr=subprocess.PIPE, check=False)
    if run.returncode != 0 or partial.stat().st_size == 0:
        partial.unlink(missing_ok=True)
        detail = run.stderr.decode(errors="replace").strip().splitlines()
        raise RuntimeError(f"MAFFT failed: {detail[-1] if detail else run.returncode}")
    os.replace(partial, target)
    return {
        "ops": [],
        "message": f"Aligned {count} sequences",
        "outputs": [str(target)],
        "tool": {"name": "mafft", "version": mafft_version(exe)},
        "execution": {"argv": argv, "resolved_parameters": {"threads": threads, "preserve_case": True}, "preprocessing": [], "environment": {}, "methods_template": "mafft.align.v1", "methods": {"description": "Sequences were aligned", "citations": ["Katoh and Standley (2013), MAFFT version 7. https://doi.org/10.1093/molbev/mst010"]}},
    }


@plugin.operation("place_sanger")
def place_sanger(snapshot, params):
    """Map a draft Sanger fragment to columns in a frozen source MSA."""
    from sanger_place import place

    source = Path(params["input"])
    target = Path(params["output"])
    result = place(source, str(params["fragment"]), target, mafft_executable(), threads=params.get("threads"))
    return {"ops": [], "outputs": [str(target)],
            "message": f"Placed {len(result['fragment'])} bases; "
                       f"{result['insertions']} insertion anchors",
            "tool": {"name": "mafft", "version": result["version"]}}


@plugin.operation("add_sequences")
def add_sequences(snapshot, params):
    from add_sequences import run_add

    result = run_add(Path(params["input"]), params["rows"], Path(params["output"]),
                     mafft_executable(), orientation=bool(params.get("orientation", False)),
                     keep=bool(params.get("keep", True)), threads=params.get("threads"))
    return {"ops": [], "outputs": [params["output"]],
            "message": f"Placed {len(result['rows'])} new sequences",
            "tool": {"name": "mafft", "version": mafft_version(mafft_executable())}}


if __name__ == "__main__":
    plugin.run()
