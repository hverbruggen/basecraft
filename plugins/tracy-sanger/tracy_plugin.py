"""Tracy consensus through Basecraft's public file-operation plugin API."""

from pathlib import Path

from basecraft.plugin import Plugin
from basecraft.tools import allocated_cpus, executable
from consensus import file_hash, merge, trace_calls

plugin = Plugin("basecraft.tracy-sanger", "0.2.0")


@plugin.operation("consensus")
def consensus(snapshot, params, context):
    reads = [Path(value) for value in context.inputs["reads"]]
    if len(reads) not in (1, 2, 3):
        raise ValueError(
            "Exactly 1, 2 or 3 Sanger reads are required; larger assemblies are unsupported"
        )
    target = Path(context.output("artifacts"))
    tracy = executable(("tracy",), "BASECRAFT_TRACY")
    if params.get("engine_sha256") and params["engine_sha256"] != file_hash(
        Path(tracy)
    ):
        raise ValueError("Tracy changed since this draft; regenerate explicitly")
    engine_hash = file_hash(Path(tracy))
    input_hashes = [file_hash(path) for path in reads]
    report = merge(
        reads,
        Path(tracy),
        target,
        minimum_overlap=int(params.get("minimum_overlap", 50)),
        minimum_identity=float(params.get("minimum_identity", 0.8)),
        signal_threshold=int(params.get("signal_threshold", 20)),
        workers=allocated_cpus(),
    )
    if file_hash(Path(tracy)) != engine_hash or report["engine_sha256"] != engine_hash:
        raise ValueError("Tracy changed during calling; results were not accepted")
    if [file_hash(path) for path in reads] != input_hashes or [
        item["sha256"] for item in report["inputs"]
    ] != input_hashes:
        raise ValueError("Sanger input changed during calling; results were not accepted")
    return {
        "ops": [],
        "message": f"Draft consensus: {report['consensus_length']} bases, "
        f"{report['conflict_columns']} conflicting columns",
        "tool": {"name": "Tracy", "version": report["version_output"]},
    }


@plugin.operation("calls")
def calls(snapshot, params, context):
    import json

    target = Path(context.output("artifacts", "calls.json"))
    tracy = executable(("tracy",), "BASECRAFT_TRACY")
    expected = params.get("engine_sha256")
    if expected and expected != file_hash(Path(tracy)):
        raise ValueError(
            "Tracy changed since this draft; regenerate the draft before reviewing"
        )
    data = trace_calls(
        Path(context.input("read")), Path(tracy), target.with_suffix(".trace.json")
    )
    target.write_text(json.dumps(data))
    return {
        "ops": [],
        "message": "Tracy calls loaded",
    }


@plugin.operation("primers")
def primers(snapshot, params, context):
    import json

    from primers import detect

    result = detect(
        params["sequence"],
        params.get("forward", ""),
        params.get("reverse", ""),
        int(params.get("minimum_overlap", 12)),
        int(params.get("error_percent", 25)),
    )
    target = Path(context.output("artifacts", "primers.json"))
    target.write_text(json.dumps(result, indent=2) + "\n")
    return {
        "ops": [],
        "message": f"Found {len(result['hits'])} primer candidates",
    }


if __name__ == "__main__":
    plugin.run()
