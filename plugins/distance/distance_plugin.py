"""Public file-operation adapter for the shared Rust distance engine."""
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import write_atomic

CACHE_MAGIC = b"BASECRAFT-DISTANCES-1\n"

plugin = Plugin("basecraft.distance", "0.1.0")


MEMBERS = {"tree": "tree.nwk", "matrix": "distances.tsv", "groups": "groups.tsv"}


def executable():
    path = os.environ.get("BASECRAFT_DISTANCE")
    if path:
        return str(path)
    found = shutil.which("basecraft-distance")
    if found:
        return found
    raise FileNotFoundError("Install basecraft-distance or set BASECRAFT_DISTANCE")


def retained_output(metadata, expected):
    """Never register an engine-returned file outside the allocated collection."""
    report = metadata.get("bootstrap")
    collection = report.get("replicate_trees") if isinstance(report, dict) else None
    if not isinstance(collection, dict):
        raise RuntimeError("Distance engine did not report the requested bootstrap collection")  # noqa: TRY004 - invalid engine response
    counts = [report.get(key) for key in ("requested", "attempted", "completed", "failed_replicates")]
    if (any(type(value) is not int or value < 0 for value in counts)
            or not counts[0] >= counts[1] == counts[2] + counts[3]
            or type(report.get("support_withheld")) is not bool
            or type(report.get("cancelled")) is not bool):
        raise RuntimeError("Distance engine returned invalid bootstrap artifact counts or flags")
    actual = Path(collection.get("path", ""))
    path = expected.with_suffix(".partial.nex") if report["support_withheld"] else expected
    status = ("cancelled" if report["cancelled"] else "original_failed" if report["attempted"] == 0
              else "partial" if report["support_withheld"] else "complete")
    if (actual != path or actual.is_symlink() or not actual.is_file() or actual.resolve() != actual
            or collection.get("format") != "nexus" or collection.get("status") != status
            or type(collection.get("trees")) is not int or collection["trees"] != report["completed"]
            or (not report["support_withheld"] and (report["completed"] != report["requested"] or report["cancelled"]))):
        raise RuntimeError("Distance engine returned an invalid bootstrap artifact path or status")
    return str(actual)


def recovery_error(diagnostic, metadata, report, retained, save_report=True):
    """Preserve only already-validated publication evidence from this invocation."""
    failed = dict(metadata, output_status="failed", error=diagnostic)
    if save_report:
        try:
            write_atomic(report, json.dumps(failed, indent=2) + "\n")
        except OSError as error:
            diagnostic += f"; failed run report could not be saved: {error}"
    if retained:
        evidence = failed["bootstrap"]["replicate_trees"]
        diagnostic += f"; retained bootstrap artifacts: {json.dumps(evidence)}; report: {report}"
    return RuntimeError(diagnostic)


NJ = ["Saitou N, Nei M. 1987. The neighbor-joining method: a new method for reconstructing phylogenetic trees. Mol Biol Evol 4:406-425.",
      "Studier JA, Keppler KJ. 1988. A note on the neighbor-joining algorithm of Saitou and Nei. Mol Biol Evol 5:729-731."]
FAST_NJ = ["Clausen PTLC. 2023. Scaling neighbor joining to one million taxa with dynamic and heuristic neighbor joining. Bioinformatics 39:btac774."]
INCOMPLETE = ["Criscuolo A, Gascuel O. 2008. Fast NJ-like algorithms to deal with incomplete distance matrices. BMC Bioinformatics 9:166."]
COUNTING = ["Elias I, Lagergren J. 2007. Fast computation of distance estimators. BMC Bioinformatics 8:89.",
            "Simonsen M, Pedersen CNS. 2011. Rapid computation of distance estimators from nucleotide and amino acid alignments. Proc ACM Symposium on Applied Computing, 89-93."]
MODELS = {"p": ("uncorrected p-distances", []),
          "jc69": ("Jukes-Cantor distances", ["Jukes TH, Cantor CR. 1969. Evolution of protein molecules. In: Munro HN, editor. Mammalian Protein Metabolism. New York: Academic Press. p 21-132."]),
          "k2p": ("Kimura two-parameter distances", ["Kimura M. 1980. A simple method for estimating evolutionary rates of base substitutions through comparative studies of nucleotide sequences. J Mol Evol 16:111-120."])}
TREES = {"sorted": ("exact neighbor joining (Saitou and Nei 1987; Studier and Keppler 1988), searched with row bounds derived from dynamic neighbor joining (Clausen 2023) and extended to non-metric distances", NJ + FAST_NJ),
         "njstar": ("NJ* (Criscuolo and Gascuel 2008)", NJ + FAST_NJ + INCOMPLETE),
         "mvrstar": ("MVR* (Criscuolo and Gascuel 2008)", INCOMPLETE),
         "fourpoint": ("neighbor joining after a synchronous, uncapped variant of four-point completion of missing distances, without weighted least-squares refinement (Landry et al. 1996; Makarenkov and Lapointe 2004)", NJ + FAST_NJ + [
             "Landry PA, Lapointe FJ, Kirsch JAW. 1996. Estimating phylogenies from lacunose distance matrices: additive is superior to ultrametric estimation. Mol Biol Evol 13:818-823.",
             "Makarenkov V, Lapointe FJ. 2004. A weighted least-squares approach for inferring phylogenies from incomplete distance matrices. Bioinformatics 20:2113-2121."]),
         "upgma": ("UPGMA (Sokal and Michener 1958), using a nearest-neighbor chain (Murtagh 1983)", [
             "Sokal RR, Michener CD. 1958. A statistical method for evaluating systematic relationships. Univ Kansas Sci Bull 38:1409-1438.",
             "Murtagh F. 1983. A survey of recent advances in hierarchical clustering algorithms. Comput J 26:354-359."])}
JOIN_TIES = ("Joins were compared in double-precision arithmetic and ties were settled by input order, "
             "so tied parts of the tree can depend on sequence order")
INCOMPLETE_TIES = ("Joins were compared in double-precision arithmetic. At steps with incomplete distances, "
                   "a shortlist of up to 15 candidate pairs ordered by Q then input order was ranked by "
                   "the fraction of satisfied quartet comparisons, the number of comparisons, the number of "
                   "taxa with asymmetric distance availability, and the summed quartet margin, in that order "
                   "(larger scores preferred); remaining ties used shortlist order. At steps with complete "
                   "distances, the lowest Q was selected, with ties settled by input order. "
                   "Tied parts of the tree can depend on sequence order")
UPGMA_TIES = ("Merges were compared in double-precision arithmetic and ties were settled by the chain's previous "
              "cluster, then input order, so tied parts of the tree can depend on sequence order")
BOOTSTRAP = ["Felsenstein J. 1985. Confidence limits on phylogenies: an approach using the bootstrap. Evolution 39:783-791.",
             "Steele GL, Lea D, Flood CH. 2014. Fast splittable pseudorandom number generators. Proc OOPSLA, 453-472."]


def bootstrap_methods(requested, report):
    """Describe the replicates that actually ran, and say so when support was withheld."""
    if not report or not report["support_withheld"]:
        count = report["completed"] if report else requested
        return f", with {count} ordinary site bootstrap replicates (Felsenstein 1985), resampled with SplitMix64 (Steele et al. 2014)"
    outcome = "was cancelled" if report["cancelled"] else "was incomplete"
    return (f". Ordinary site bootstrap (Felsenstein 1985), resampled with SplitMix64 (Steele et al. 2014), {outcome} "
            f"({report['requested']} replicates requested, {report['attempted']} attempted, {report['completed']} completed, "
            f"{report['failed_replicates']} failed), so bootstrap support was withheld from the tree")


def methods(kind, model, method, min_overlap, bootstrap, dynamic_search=True, bootstrap_report=None):
    """Scientific description and citations for this run, carried into the Methods text."""
    name, cited = MODELS[model]
    text = (f"Pairwise {name} were calculated with basecraft-distance, excluding gaps and ambiguous sites "
            f"pairwise, with a minimum overlap of {min_overlap}, using packed bit-parallel site counts "
            "(Elias and Lagergren 2007; Simonsen and Pedersen 2011)")
    citations = [*cited, *COUNTING]
    if kind == "tree":
        tree_text, tree_cited = TREES[method]
        if method == "sorted" and not dynamic_search:
            tree_text = "exact neighbor joining (Saitou and Nei 1987; Studier and Keppler 1988)"
        text += f". The tree was inferred by {tree_text}"
        # NJ* only reaches the dynamic search for components with complete distances.
        citations += [c for c in tree_cited if dynamic_search or c not in FAST_NJ]
        if bootstrap:
            text += bootstrap_methods(bootstrap, bootstrap_report)
            citations += BOOTSTRAP
        tie_text = UPGMA_TIES if method == "upgma" else INCOMPLETE_TIES if method in ("njstar", "mvrstar") else JOIN_TIES
        text += f". {tie_text}"
    return {"description": text, "citations": citations}


def used_dynamic_search(metadata):
    details = (metadata.get("tree") or {}).get("details") or []
    return any("q_evaluations" in detail for detail in details)


def resolved_parameters(kind, model, method, params, bootstrap):
    resolved = {"model": model, "min_overlap": params.get("min_overlap", 1)}
    if kind == "tree":
        resolved["method"] = method
        resolved["bootstrap"] = bootstrap
        if bootstrap:
            resolved["seed"] = params.get("seed", 1)
    return resolved


def cache_identity(path):
    """Producer evidence from a compact cache's header, or from an older JSON cache."""
    try:
        with Path(path).open("rb") as stream:
            magic = stream.read(len(CACHE_MAGIC))
            saved = json.loads(stream.readline() if magic == CACHE_MAGIC else magic + stream.read())
    except (OSError, ValueError) as error:
        raise ValueError("Distance cache input can't be read; calculate a fresh cache") from error
    if not isinstance(saved, dict):
        raise ValueError("Distance cache input can't be read; calculate a fresh cache")  # noqa: TRY004
    return saved.get("basecraft_identity")


def invoke(kind, params, context, target=None):
    """Run the engine on the allocated alignment; results go to `artifacts`."""
    target = Path(target or context.output("artifacts", MEMBERS[kind]))
    source = Path(context.input("alignment"))
    model = params.get("model", "k2p")
    method = params.get("method", "sorted" if params.get("bootstrap", 0) else "njstar")
    if model not in ("p", "jc69", "k2p"):
        raise ValueError("Model must be p, jc69, or k2p")
    if method not in ("sorted", "fourpoint", "njstar", "mvrstar", "upgma"):
        raise ValueError("Unknown tree method")
    bootstrap = params.get("bootstrap", 0) if kind == "tree" else 0
    if not isinstance(bootstrap, int) or isinstance(bootstrap, bool) or bootstrap < 0:
        raise ValueError("Bootstrap must be a nonnegative integer")
    if bootstrap and method not in ("sorted", "upgma"):
        raise ValueError("Bootstrap supports only sorted NJ and UPGMA")
    collection = Path(context.output("artifacts", f"bootstrap-{context.run_id}.nex")) if bootstrap else None
    cache = target.with_name(target.name + ".distances.bin")
    producer = Path(executable()).resolve()
    def digest(path):
        with Path(path).open("rb") as stream:
            return hashlib.file_digest(stream, "sha256").hexdigest()
    identity = {"schema": 1, "input_sha256": digest(source), "engine_sha256": digest(producer),
                "model": model, "min_overlap": params.get("min_overlap", 1)}
    supplied_cache = context.input("cache")
    if supplied_cache:
        supplied = Path(supplied_cache)
        if cache_identity(supplied) != identity:
            raise ValueError("Distance cache input/model/effective parameters/producer do not match; calculate a fresh cache")
        if cache.exists():
            raise ValueError("Allocated distance cache already exists")
        shutil.copyfile(supplied, cache)
    report = Path(str(target) + ".run.json")
    with tempfile.TemporaryDirectory(prefix="basecraft-distance-") as work:
        staged = Path(work) / "result"
        args = [str(producer), "--cache", str(cache), "--cache-identity", json.dumps(identity), "--model", model,
                "--method", method, "--min-overlap", str(params.get("min_overlap", 1))]
        if kind == "groups":
            # input is the alignment; assignments are separate and the cache is reusable.
            assignments = Path(work) / "groups.json"
            if "groups_data" in params:
                assignments.write_text(json.dumps(params["groups_data"]))
            else:
                shutil.copyfile(context.input("groups"), assignments)
            args += ["--input", str(source), "--groups", str(assignments),
                     "--summary", str(staged)]
        else:
            args += ["--input", str(source), "--" + kind, str(staged)]
        if bootstrap:
            args += ["--bootstrap", str(bootstrap), "--seed", str(params.get("seed", 1)),
                     "--bootstrap-trees", str(collection)]
            args += ["--progress", context.progress_file, "--cancel-file", context.cancel_file]
        for name in ("threads", "tree_threads"):
            if name in params:
                args += ["--" + name.replace("_", "-"), str(params[name])]
        run = subprocess.run(args, capture_output=True, text=True, check=False)
        if run.returncode:
            # Preserve structured diagnostics even when the original matrix has no tree.
            # The host currently logs completed tool runs only.
            retained = []
            failed = None
            if run.stdout.strip():
                try:
                    failed = json.loads(run.stdout)
                    if collection:
                        retained = [retained_output(failed, collection)]
                except ValueError:
                    failed = None
            diagnostic = run.stderr.strip() or "Distance calculation failed"
            if failed is not None:
                raise recovery_error(diagnostic, failed, report, retained)
            raise RuntimeError(diagnostic)
        metadata = json.loads(run.stdout)
        if supplied_cache and not metadata.get("cache_reused"):
            raise ValueError("Distance engine refused the supplied cache identity; no result was published")
        retained = [retained_output(metadata, collection)] if collection else []
        # Publish on the destination filesystem, including for cross-device temp dirs.
        partial = None
        try:
            with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary:
                partial = Path(temporary.name)
            shutil.copyfile(staged, partial)
            os.replace(partial, target)
        except OSError as error:
            diagnostic = f"Primary output publication failed: {error}"
            if partial is not None:
                try:
                    partial.unlink(missing_ok=True)
                except OSError as cleanup_error:
                    diagnostic += f"; primary scratch cleanup failed at {partial}: {cleanup_error}"
            raise recovery_error(diagnostic, metadata, report, retained) from error
    try:
        write_atomic(report, json.dumps(metadata, indent=2) + "\n")
    except OSError as error:
        raise recovery_error(f"Run report publication failed: {error}", metadata, report, retained, save_report=False) from error
    extra_trees = []
    count = len(metadata["tree"]["trees"]) if metadata["tree"] else 0
    if count > 1:
        for i, newick in enumerate(metadata["tree"]["trees"], 1):
            path = target.with_name(f"{target.stem}.component-{i}.nwk")
            path.write_text(newick)
            extra_trees.append(str(path))
    message = f"Built {count} distance tree(s)" if kind == "tree" else "Calculated distances" if kind == "matrix" else "Summarized groups"
    bootstrap_report = metadata.get("bootstrap")
    if bootstrap_report:
        if bootstrap_report["support_withheld"]:
            message += "; bootstrap support withheld: " + "; ".join(bootstrap_report["failures"])
        else:
            message += f"; {bootstrap_report['completed']} bootstrap replicates (seed {bootstrap_report['seed']})"
        message += f"; bootstrap trees: {retained[0]}"
    return {"ops": [], "message": message,
            "tool": {"name": "basecraft-distance", "version": metadata["version"]},
            "execution": {"argv": args, "resolved_parameters": resolved_parameters(kind, model, method, params, bootstrap),
                          "preprocessing": ["Gaps and ambiguous sites excluded pairwise"], "environment": {},
                          "methods_template": f"basecraft.distance.{kind}.v1", "methods": methods(kind, model, method, params.get("min_overlap", 1), bootstrap, used_dynamic_search(metadata), bootstrap_report)}}


@plugin.operation("tree")
def tree(params, context):
    return invoke("tree", params, context)


@plugin.operation("matrix")
def matrix(params, context):
    return invoke("matrix", params, context)


@plugin.operation("groups")
def groups(params, context):
    return invoke("groups", params, context)


if __name__ == "__main__":
    plugin.run()
