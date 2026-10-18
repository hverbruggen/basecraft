"""IQ-TREE with explicit seed, model, worker count and branch support."""

import json
import re
import subprocess
import tempfile
import time
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import (
    allocated_cpus,
    bootstrap_collection_path,
    executable,
    quote_newick,
    read_alignment,
    write_atomic,
)
from output_contract import publish_artifacts, restore_tree

plugin = Plugin("basecraft.iqtree", "0.1.0")


@plugin.operation("tree")
def tree(snapshot, params):
    source, target = Path(params["input"]).resolve(), Path(params["output"]).resolve()
    if source == target:
        raise ValueError("Output must differ from input")
    def cancelled():
        return params.get("cancel_file") and Path(params["cancel_file"]).exists()

    if cancelled():
        raise RuntimeError("IQ-TREE cancelled before inference")
    records = read_alignment(source, minimum=4)
    model = params.get("model", "MFP")
    if not isinstance(model, str) or not re.fullmatch(r"[A-Za-z0-9+{},._/-]+", model):
        raise ValueError("Invalid IQ-TREE model")
    # A base model takes rate variation from the settings form's switches;
    # a full model string such as GTR+I+G is used as given.
    if model != "MFP" and "+" not in model:
        model += ("+I" if params.get("invariant", False) else "") + ("+G" if params.get("gamma", False) else "")
    threads, seed = allocated_cpus(params.get("threads")), int(params.get("seed", 1))
    if threads < 1 or seed < 1:
        raise ValueError("Threads and seed must be positive")
    support = params.get("support", "none")
    if support not in ("none", "ufboot", "bootstrap"):
        raise ValueError("Support must be none, ufboot or bootstrap")
    replicates = int(params.get("replicates", 1000 if support == "ufboot" else 100))
    if support == "ufboot" and replicates < 1000:
        raise ValueError("Ultrafast bootstrap needs at least 1000 replicates")
    if support == "bootstrap" and replicates < 1:
        raise ValueError("Bootstrap needs at least one replicate")
    support_text = {"none": "not requested", "ufboot": f"ultrafast bootstrap, {replicates} replicates", "bootstrap": f"standard bootstrap, {replicates} replicates"}[support]
    exe = executable(("iqtree3", "iqtree2", "iqtree"), "BASECRAFT_IQTREE")
    outputs = []
    selected_model = None
    with tempfile.TemporaryDirectory(prefix="basecraft-iqtree-") as work:
        renamed, prefix = Path(work) / "input.fa", Path(work) / "analysis"
        renamed.write_text("".join(f">t{i}\n{s}\n" for i, (_, s) in enumerate(records)))
        args = [
            exe,
            "-s",
            str(renamed),
            "-pre",
            str(prefix),
            "-m",
            model,
            "-nt",
            str(threads),
            "-seed",
            str(seed),
            "-quiet",
        ]
        if params.get("keep_identical", False):
            args.append("-keep-ident")
        if support != "none":
            # The tree file then carries the support values on its branches.
            args += ["-B" if support == "ufboot" else "-b", str(replicates)]
        # Files avoid a pipe-buffer deadlock during long external runs.
        with (Path(work) / "stdout").open("w+") as stdout, (Path(work) / "stderr").open("w+") as stderr:
            run = subprocess.Popen(args, stdout=stdout, stderr=stderr)
            while run.poll() is None:
                if cancelled():
                    run.terminate()
                    try:
                        run.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        run.kill()
                        run.wait()
                    raise RuntimeError("IQ-TREE cancelled; no completed display tree")
                time.sleep(0.1)
            stdout.seek(0)
            stderr.seek(0)
            diagnostic = stderr.read().strip() or stdout.read().strip()
        if cancelled():
            raise RuntimeError("IQ-TREE cancelled; no completed display tree")
        treefile = prefix.with_suffix(".treefile")
        if run.returncode or not treefile.exists():
            raise RuntimeError(
                diagnostic or "IQ-TREE failed"
            )
        names = {f"t{i}": h.split()[0] for i, (h, _) in enumerate(records)}
        newick = restore_tree(treefile.read_text().strip(), names)
        artifacts = [(target, newick + "\n")]
        for suffix in (".iqtree", ".log"):
            report = prefix.with_suffix(suffix)
            if report.exists():
                report_text = report.read_text()
                if suffix == ".iqtree":
                    chosen = re.search(r"^Best-fit model according to BIC:\s*(\S+)", report_text, re.MULTILINE)
                    if chosen:
                        selected_model = chosen.group(1)
                artifacts.append((Path(str(target) + suffix), report_text))
            else:
                artifacts.append((Path(str(target) + suffix), None))
        artifacts.append(
            (
                Path(str(target) + ".run.json"),
                json.dumps(
                    {
                        "model": model,
                        "threads": threads,
                        "seed": seed,
                        "identifiers": names,
                        "support": support_text,
                        "keep_identical": params.get("keep_identical", False),
                        "tree_role": params.get("tree_role", "phylogenetic inference"),
                        "units": "substitutions per site",
                        "command": args,
                    },
                    indent=2,
                )
                + "\n",
            )
        )
        # Standard bootstrap replicate trees, saved where the run's
        # bootstrap_destination says, with the original names restored.
        boottrees = prefix.with_suffix(".boottrees")
        replicate_trees = None
        if support == "bootstrap" and boottrees.exists():
            replicate_trees = bootstrap_collection_path(params)
            lines = [line.strip() for line in boottrees.read_text().splitlines() if line.strip()]
            # Replicates may leave out identical sequences, so names are
            # restored label by label rather than checked as a full set.
            label = re.compile(r"(?<=[(,])(t\d+)(?=[:,)])")
            nexus = "#NEXUS\nbegin trees;\n" + "".join(
                f"  tree replicate_{i} = {label.sub(lambda m: quote_newick(names.get(m.group(1), m.group(1))), line)}\n"
                for i, line in enumerate(lines, 1)
            ) + "end;\n"
        if any(path.resolve() == source for path, _ in artifacts):
            raise ValueError("An output would overwrite the input")
        version = subprocess.run(
            [exe, "--version"], capture_output=True, text=True, check=False
        )
        if cancelled():
            raise RuntimeError("IQ-TREE cancelled before publication")
        publish_artifacts(artifacts)
        outputs = [str(path) for path, payload in artifacts if payload is not None]
        if replicate_trees is not None:
            write_atomic(replicate_trees, nexus)
            outputs.append(str(replicate_trees))
    return {
        "ops": [],
        "outputs": outputs,
        "message": f"Built an IQ-TREE tree of {len(records)} sequences ({model})",
        "execution": {"argv": args, "resolved_parameters": {"requested_model": model, "selected_model": selected_model, "threads": threads, "seed": seed, "support": support_text, "keep_identical": params.get("keep_identical", False), "tree_role": params.get("tree_role", "phylogenetic inference"), "units": "substitutions per site"}, "preprocessing": ["Sequence identifiers mapped to neutral t0..tN identifiers and restored in Newick"], "environment": {}, "methods_template": "iqtree.tree.v1", "methods": {"description": "A phylogenetic tree was inferred", "citations": ["IQ-TREE citations by software version and method: https://www.iqtree.org/doc/Home"]}},
        "tool": {
            "name": "IQ-TREE",
            "version": (version.stdout or version.stderr).strip(),
        },
    }


if __name__ == "__main__":
    plugin.run()
