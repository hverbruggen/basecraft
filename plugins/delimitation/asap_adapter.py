"""Complete upstream ASAP fallback; native default remains behind validation gate."""
import json
import math
import os
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET
from datetime import UTC, datetime
from pathlib import Path

MISSING = "ASAP engine is not installed; build it with tools/install-asap.py or set BASECRAFT_ASAP_PYTHON"
PROBE = """
import importlib.util, sys
def found(name):
    try:
        return importlib.util.find_spec(name) is not None
    except ModuleNotFoundError:
        return False
sys.exit(0 if found("itaxotools.asapy.asap") or found("asap") else 1)
"""


def engine_problem():
    """Why the upstream ASAP engine cannot run here, or None."""
    python = os.environ.get("BASECRAFT_ASAP_PYTHON", sys.executable)
    try:
        found = subprocess.run([python, "-c", PROBE], check=False, capture_output=True, timeout=60).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        found = False
    return None if found else MISSING


def invoke(params, native, run_native):
    if params.get("engine", "upstream") != "upstream":
        raise ValueError("Native ASAP has not passed the full scoring and performance gates; use upstream")
    source = Path(params["input"]).resolve()
    target = Path(params["output"]).resolve()
    directory = Path(str(target) + ".asap-" + params.get("job_id", str(uuid.uuid4())))
    directory.mkdir(parents=True)
    # The upstream name sanitizer is lossy. Replace names with reversible tokens
    # and restore their exact original identities only after validating coverage.
    lines = source.read_text().splitlines()
    records = []
    for line in lines:
        if line.startswith(">"):
            records.append((line[1:].split()[0], []))
        elif line.strip():
            if not records:
                raise ValueError("ASAP requires an aligned DNA FASTA input")
            records[-1][1].append(line.strip())
    names = [name for name, _ in records]
    sequences = ["".join(parts).upper().replace("U", "T") for _, parts in records]
    if len(records) < 2 or len(set(names)) != len(names) or len({len(s) for s in sequences}) != 1 or not sequences[0]:
        raise ValueError("ASAP requires at least two unique identifiers and a nonempty rectangular alignment")
    if any(set(s) - set("ACGTRYSWKMBDHVN?-") for s in sequences):
        raise ValueError("ASAP input contains unsupported DNA symbols")
    mapping = {f"specimen_{i:09d}": name for i, name in enumerate(names)}
    input_file = directory / "input.fasta"
    input_file.write_text("".join(f">{token}\n{sequence}\n" for token, sequence in zip(mapping, sequences)))
    model = params.get("model", "p")
    models = {"k2p": 0, "jc69": 1, "tn93": 2, "p": 3}
    if model not in models:
        raise ValueError("ASAP model must be p, jc69, k2p or tn93")
    replicates = params.get("replicates", 10000)
    seed = params.get("seed", 1)
    if not isinstance(replicates, int) or not 100 <= replicates <= 10000000 or not isinstance(seed, int) or not 1 <= seed <= 2147483647:
        raise ValueError("Invalid ASAP simulation count or seed")
    results = directory / "results"
    results.mkdir()
    request = {"input": str(input_file), "out": str(results) + os.sep,
                   "time": datetime.now(UTC).isoformat(timespec="seconds"), "number": params.get("partitions", 10),
                   "method": models[model], "seed": seed, "sequence_length": len(sequences[0]),
                   "replicates": replicates, "seuil_pvalue": 0.001, "pond_pente": 0.5, "rate": 2.0, "all": True}
    request["worker_metadata"] = str(directory / "upstream.json")
    request_path = directory / "request.json"
    request_path.write_text(json.dumps(request))
    command = [os.environ.get("BASECRAFT_ASAP_PYTHON", sys.executable), str(Path(__file__).with_name("asap_worker.py")), str(request_path)]
    with (directory/"stdout.log").open("w") as stdout, (directory/"stderr.log").open("w") as stderr:
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        while process.poll() is None:
            if params.get("cancel_file") and Path(params["cancel_file"]).exists():
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise RuntimeError("ASAP cancelled; no completed hypotheses")
            time.sleep(0.1)
    if process.returncode:
        raise RuntimeError((directory/"stderr.log").read_text()[-16000:] or "ASAP failed")
    files = list(results.glob("*.spart.xml"))
    if len(files) != 1:
        raise ValueError("ASAP did not emit exactly one SPART-XML result")
    xml = ET.parse(files[0])
    observed = [node.attrib["id"] for node in xml.findall("./individuals/individual")]
    if set(observed) != set(mapping) or len(observed) != len(mapping):
        raise ValueError("ASAP roster differs from its immutable input")
    for node in xml.findall("./individuals/individual"):
        node.set("id", mapping[node.attrib["id"]])
    for partition in xml.findall("./spartitions/spartition"):
        assigned = [node.attrib["ref"] for node in partition.findall("./subsets/subset/individual")]
        if len(assigned) != len(mapping) or set(assigned) != set(mapping):
            raise ValueError("ASAP partition has missing or overlapping memberships")
        for node in partition.findall("./subsets/subset/individual"):
            node.set("ref", mapping[node.attrib["ref"]])
        score = float(partition.attrib["spartitionScore"])
        if not math.isfinite(score):
            raise ValueError("ASAP emitted nonfinite score")
    restored = directory / "restored.spartxml"
    xml.write(restored, encoding="utf-8", xml_declaration=True)
    upstream = json.loads((directory / "upstream.json").read_text())
    result = run_native("asap_import", dict(params, input=str(restored), original_source=params.get("original_source", str(source)),
                                           asap_model=model, replicates=replicates, public_method="asap", upstream_version=upstream["version"], upstream=upstream, command=command))
    result["tool"] = {"name": "ASAP upstream", "version": upstream["version"]}
    result["outputs"] += [str(path) for path in directory.rglob("*") if path.is_file()]
    return result
