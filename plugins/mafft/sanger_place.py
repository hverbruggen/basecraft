"""Place one review-only Sanger draft in a frozen DNA alignment with MAFFT.

MAFFT --addfragments --mapout supplies fragment-to-source-column coordinates.
--mapout implies --keeplength; inserted fragment bases remain in our mapping
as null, while the original source alignment is never modified.
"""

import hashlib
import json
import subprocess
import tempfile
from itertools import pairwise
from pathlib import Path

from basecraft.tools import allocated_cpus, read_alignment, write_atomic


def place(
    alignment: Path, fragment: str, output: Path, mafft: str, threads=None
) -> dict:
    fragment = fragment.upper().replace("-", "")
    if (
        not fragment
        or len(fragment) > 100_000
        or set(fragment) - set("ACGTNRYKMSWBDHV")
    ):
        raise ValueError("Sanger fragment must be nonempty IUPAC DNA")
    resolved_threads = allocated_cpus(threads)
    source = read_alignment(alignment)
    if len(source) < 2:
        raise ValueError("A source alignment needs at least two rows")
    width = len(source[0][1])
    if width == 0 or any(len(seq) != width for _, seq in source):
        raise ValueError("Source alignment is empty or not rectangular")
    if any(set(seq.upper()) - set("ACGTNRYKMSWBDHV-.?") for _, seq in source):
        raise ValueError("Source alignment contains non-DNA symbols")
    if output.exists():
        raise ValueError(f"Placement output already exists: {output}")
    with tempfile.TemporaryDirectory(prefix="basecraft-mafft-sanger-") as directory:
        work = Path(directory)
        original = work / "alignment.fa"
        probe = work / "probe.fa"
        original.write_text(
            "".join(f">r{i}\n{seq}\n" for i, (_, seq) in enumerate(source))
        )
        probe.write_text(f">basecraft_probe\n{fragment}\n")
        run = subprocess.run(
            [
                mafft,
                "--thread",
                str(resolved_threads),
                "--auto",
                "--addfragments",
                str(probe),
                "--mapout",
                str(original),
            ],
            cwd=work,
            capture_output=True,
            text=True,
            timeout=300,
            check=False,
        )
        if run.returncode:
            raise RuntimeError(f"MAFFT placement failed: {run.stderr[-2000:]}")
        placed = work / "placed.fa"
        placed.write_text(run.stdout)
        result_rows = read_alignment(placed)
        rows = dict(result_rows)
        if len(rows) != len(result_rows):
            raise ValueError("MAFFT duplicated an alignment row")
        if set(rows) != {*(f"r{i}" for i in range(len(source))), "basecraft_probe"}:
            raise ValueError("MAFFT omitted or duplicated a source alignment row")
        if any(
            rows[f"r{i}"].upper() != source[i][1].upper() for i in range(len(source))
        ):
            raise ValueError("MAFFT changed the frozen source alignment")
        if len(rows["basecraft_probe"]) != width:
            raise ValueError("MAFFT changed source alignment width")
        mapping = []
        map_file = Path(str(probe) + ".map")
        if not map_file.exists():
            raise ValueError("MAFFT did not write fragment-to-alignment map")
        for line in map_file.read_text().splitlines():
            if not line or line.startswith((">", "#")):
                continue
            fields = [item.strip() for item in line.split(",")]
            if len(fields) != 3 or fields[1] != str(len(mapping) + 1):
                raise ValueError(f"Malformed MAFFT mapping row: {line}")
            base, _, column = fields
            if base.upper() != fragment[len(mapping)]:
                raise ValueError("MAFFT mapping changed a fragment base")
            if column == "-":
                mapping.append(None)
            else:
                mapped = int(column) - 1
                if mapped < 0 or mapped >= width:
                    raise ValueError("MAFFT mapped outside source alignment")
                mapping.append(mapped)
        if len(mapping) != len(fragment):
            raise ValueError("MAFFT omitted fragment positions from its map")
        observed = "".join(
            fragment[i] for i, column in enumerate(mapping) if column is not None
        )
        placed_bases = rows["basecraft_probe"].replace("-", "").upper()
        if observed != placed_bases:
            raise ValueError("MAFFT placement and coordinate map disagree")
        if not any(column is not None for column in mapping):
            raise ValueError("No fragment bases placed against source alignment")
        placed_columns = [column for column in mapping if column is not None]
        if any(a >= b for a, b in pairwise(placed_columns)):
            raise ValueError("MAFFT mapping is not strictly monotone")
        payload = {
            "status": "candidate_requires_review",
            "engine": "MAFFT",
            "version": "unknown",
            "source_path": str(alignment.resolve()),
            "source_sha256": hashlib.sha256(alignment.read_bytes()).hexdigest(),
            "source_rows": len(source),
            "source_width": width,
            "fragment": fragment,
            "map": mapping,
            "aligned_probe": rows["basecraft_probe"].upper(),
            "insertions": sum(column is None for column in mapping),
            "parameters": ["--thread", str(resolved_threads), "--auto", "--addfragments", "--mapout"],
        }
        version = subprocess.run(
            [mafft, "--version"],
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
        payload["version"] = (version.stderr or version.stdout).strip().split()[0]
        write_atomic(output, json.dumps(payload, indent=2) + "\n")
        return payload
