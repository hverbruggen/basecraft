"""IQ-TREE with explicit seed, model and worker count; no implicit support run."""
import json
import re
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import executable, quote_newick, read_alignment, write_atomic

plugin = Plugin("basecraft.iqtree", "0.1.0")


@plugin.operation("tree")
def tree(snapshot, params):
    source, target = Path(params["input"]).resolve(), Path(params["output"]).resolve()
    if source == target:
        raise ValueError("Output must differ from input")
    records = read_alignment(source, minimum=4)
    model = params.get("model", "MFP")
    if not isinstance(model, str) or not re.fullmatch(r"[A-Za-z0-9+{},._/-]+", model):
        raise ValueError("Invalid IQ-TREE model")
    threads, seed = int(params.get("threads", 4)), int(params.get("seed", 1))
    if threads < 1 or seed < 1:
        raise ValueError("Threads and seed must be positive")
    exe = executable(("iqtree3", "iqtree2", "iqtree"), "BASECRAFT_IQTREE")
    outputs = []
    with tempfile.TemporaryDirectory(prefix="basecraft-iqtree-") as work:
        renamed, prefix = Path(work) / "input.fa", Path(work) / "analysis"
        renamed.write_text("".join(f">t{i}\n{s}\n" for i, (_, s) in enumerate(records)))
        args = [exe, "-s", str(renamed), "-pre", str(prefix), "-m", model,
                "-nt", str(threads), "-seed", str(seed), "-quiet"]
        run = subprocess.run(args, capture_output=True, text=True, check=False)
        treefile = prefix.with_suffix(".treefile")
        if run.returncode or not treefile.exists():
            raise RuntimeError(run.stderr.strip() or run.stdout.strip() or "IQ-TREE failed")
        names = {f"t{i}": h.split()[0] for i, (h, _) in enumerate(records)}
        newick = treefile.read_text().strip()
        if not newick.endswith(";"):
            raise RuntimeError("IQ-TREE produced an incomplete tree")
        newick = re.sub(r"(?<=[(,])(t\d+)(?=[:,);])",
                        lambda m: quote_newick(names[m.group(1)]), newick)
        artifacts = [(target, newick + "\n")]
        for suffix in (".iqtree", ".log"):
            report = prefix.with_suffix(suffix)
            if report.exists():
                artifacts.append((Path(str(target) + suffix), report.read_text()))
        artifacts.append((Path(str(target) + ".run.json"), json.dumps({
            "model": model, "threads": threads, "seed": seed, "identifiers": names,
            "support": "not requested", "command": args}, indent=2) + "\n"))
        if any(path.resolve() == source for path, _ in artifacts):
            raise ValueError("An output would overwrite the input")
        for path, text in artifacts:
            write_atomic(path, text)
            outputs.append(str(path))
    version = subprocess.run([exe, "--version"], capture_output=True, text=True, check=False)
    return {"ops": [], "outputs": outputs,
            "message": f"Built an IQ-TREE tree of {len(records)} sequences ({model})",
            "tool": {"name": "IQ-TREE", "version": (version.stdout or version.stderr).strip()}}


if __name__ == "__main__":
    plugin.run()
