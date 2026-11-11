"""Trim aligned columns through the public file-operation API."""
import json
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import executable, read_alignment, write_atomic

plugin = Plugin("basecraft.trimal", "0.1.0")


@plugin.operation("trim")
def trim(snapshot, params, context):
    source, target = Path(context.input("alignment")), Path(context.output("artifacts", "trimmed.afa"))
    method = params.get("method", "automated1")
    if method not in ("automated1", "gappyout", "strict", "strictplus", "nogaps"):
        raise ValueError("Unknown trimAl method")
    records = read_alignment(source)
    exe = executable(("trimal",), "BASECRAFT_TRIMAL")
    with tempfile.TemporaryDirectory(prefix="basecraft-trimal-") as work:
        renamed, output = Path(work) / "input.fa", Path(work) / "output.fa"
        renamed.write_text("".join(f">t{i}\n{s}\n" for i, (_, s) in enumerate(records)))
        argv = [exe, "-in", str(renamed), "-out", str(output), "-fasta", "-" + method]
        run = subprocess.run(argv, capture_output=True, text=True, check=False)
        write_atomic(Path(context.output("artifacts", "trimal.stderr.txt")), run.stderr)
        write_atomic(Path(context.output("artifacts", "trimal-identifiers.json")),
                     json.dumps({f"t{i}": h for i, (h, _) in enumerate(records)}, indent=2) + "\n")
        if run.returncode or not output.exists():
            raise RuntimeError(run.stderr.strip() or "trimAl produced no alignment")
        trimmed = read_alignment(output)
        original = {f"t{i}": h for i, (h, _) in enumerate(records)}
        if {h for h, _ in trimmed} != set(original):
            raise RuntimeError("trimAl changed the sequence identifiers")
        write_atomic(target, "".join(f">{original[h]}\n{s}\n" for h, s in trimmed))
    version = subprocess.run([exe, "--version"], capture_output=True, text=True, check=False)
    return {"ops": [],
            "message": f"Trimmed {len(records[0][1])} to {len(trimmed[0][1])} columns ({method})",
            "tool": {"name": "trimAl", "version": (version.stdout or version.stderr).strip()},
            "execution": {"argv": argv, "resolved_parameters": {"method": method}, "preprocessing": ["Sequence identifiers mapped to neutral t0..tN identifiers and restored after trimming"], "environment": {}, "methods_template": "trimal.trim.v1", "methods": {"description": "The alignment was trimmed", "citations": ["Capella-Gutierrez et al. (2009), trimAl. https://doi.org/10.1093/bioinformatics/btp348"]}}}


if __name__ == "__main__":
    plugin.run()
