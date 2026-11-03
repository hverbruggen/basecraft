"""Bounded, review-only Tracy consensus of two or three AB1 reads.

This research CLI does not replace chromatogram review or re-call missing peaks.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
from difflib import SequenceMatcher
from itertools import pairwise
from pathlib import Path
from statistics import median


def fasta_records(path: Path) -> list[tuple[str, str]]:
    records: list[tuple[str, str]] = []
    name = None
    parts: list[str] = []
    for line in path.read_text().splitlines():
        if line.startswith(">"):
            if name is not None:
                records.append((name, "".join(parts).upper()))
            name, parts = line[1:], []
        elif name is None:
            raise ValueError(f"Sequence before FASTA header: {path}")
        else:
            parts.append(line.strip())
    if name is not None:
        records.append((name, "".join(parts).upper()))
    if not records or any(not sequence for _, sequence in records):
        raise ValueError(f"Empty FASTA record: {path}")
    return records


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def evidence(
    rows: list[tuple[str, str]],
    consensus: str,
    minimum_overlap: int,
    minimum_identity: float,
) -> dict:
    width = len(consensus)
    if any(len(sequence) != width for _, sequence in rows):
        raise ValueError("Tracy aligned rows and consensus have different widths")
    if not re.fullmatch(r"[ACGTNRYKMSWBDHV-]+", consensus):
        raise ValueError("Tracy consensus contains unsupported symbols")
    if not re.fullmatch(r"[ACGTNRYKMSWBDHV-]+", "".join(seq for _, seq in rows)):
        raise ValueError("Tracy alignment contains unsupported symbols")
    graph: dict[int, set[int]] = {i: set() for i in range(len(rows))}
    overlaps = []
    for i in range(len(rows)):
        for j in range(i + 1, len(rows)):
            left, right = rows[i][1], rows[j][1]
            comparable = [
                (a, b) for a, b in zip(left, right) if a in "ACGT" and b in "ACGT"
            ]
            matches = sum(a == b for a, b in comparable)
            identity = matches / len(comparable) if comparable else 0.0
            accepted = (
                len(comparable) >= minimum_overlap and identity >= minimum_identity
            )
            overlaps.append(
                {
                    "reads": [i + 1, j + 1],
                    "comparable_bases": len(comparable),
                    "identity": round(identity, 5),
                    "accepted": accepted,
                }
            )
            if accepted:
                graph[i].add(j)
                graph[j].add(i)
    seen, pending = set(), [0]
    while pending:
        current = pending.pop()
        if current not in seen:
            seen.add(current)
            pending.extend(graph[current] - seen)
    if len(seen) != len(rows):
        raise ValueError("Reads do not form a connected, convincing overlap")

    first = [next((i for i, c in enumerate(seq) if c != "-"), width) for _, seq in rows]
    last = [
        next((i for i in range(width - 1, -1, -1) if seq[i] != "-"), -1)
        for _, seq in rows
    ]
    columns = []
    conflicts = 0
    for col in range(width):
        calls = []
        for row, (_, seq) in enumerate(rows):
            base = seq[col]
            if first[row] <= col <= last[row]:
                calls.append(
                    {
                        "read": row + 1,
                        "base": base,
                        "kind": "internal_gap" if base == "-" else "call",
                    }
                )
        states = {item["base"] for item in calls if item["base"] != "N"}
        conflict = len(states) > 1
        conflicts += conflict
        columns.append(
            {
                "column": col + 1,
                "consensus": consensus[col],
                "support": calls,
                "conflict": conflict,
                "review": conflict
                or len([item for item in calls if item["kind"] == "call"]) < 2,
            }
        )
    return {
        "overlaps": overlaps,
        "aligned_columns": width,
        "conflict_columns": conflicts,
        "columns": columns,
    }


def signal_bounds(raw: dict, threshold: int = 20) -> tuple[int, int]:
    """Stop at an eight-event median below a fraction of central signal.

    Inspect outwards from the middle, so isolated late noise spikes cannot
    revive a collapsed tail. This is signal visibility, not a quality score.
    """
    if not 1 <= threshold <= 90:
        raise ValueError("Signal cutoff must be between 1 and 90 percent")
    peaks = [
        max(raw["peak" + base][position - 1] for base in "ACGT")
        for position in raw["basecallPos"]
    ]
    n = len(peaks)
    if n < 16:
        return 0, n
    baseline = median(peaks[n // 4 : 3 * n // 4])
    if baseline <= 0:
        raise ValueError("Trace has no usable central signal")
    cutoff = baseline * threshold / 100
    left, right = 0, n
    for end in range(n // 2, 7, -1):
        if median(peaks[end - 8 : end]) < cutoff:
            left = end
            break
    for start in range(n // 2, n - 7):
        if median(peaks[start : start + 8]) < cutoff:
            right = start
            break
    return left, right


def trace_calls(
    path: Path, tracy: Path, output: Path, signal_threshold: int = 20
) -> dict:
    run = subprocess.run(
        [str(tracy), "basecall", "-f", "json", "-o", str(output), str(path)],
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if run.returncode:
        raise RuntimeError(f"Tracy basecall failed: {run.stderr[-2000:]}")
    raw = json.loads(output.read_text())
    if not isinstance(raw, dict):
        raise ValueError("Invalid Tracy call evidence")
    sequence = raw.get("primarySeq")
    secondary = raw.get("secondarySeq")
    samples = raw.get("basecallPos")
    scores = raw.get("basecallQual")
    if (
        not isinstance(sequence, str)
        or not re.fullmatch(r"[ACGTNRYKMSWBDHV]+", sequence.upper())
        or not isinstance(samples, list)
        or not isinstance(scores, list)
        or len(sequence) != len(samples)
        or len(sequence) != len(scores)
        or any(type(sample) is not int or not 1 <= sample <= 2**32 for sample in samples)
        or any(type(score) is not int or not -(2**31) <= score < 2**31 for score in scores)
        or any(a >= b for a, b in pairwise(samples))
        or not isinstance(raw.get("pos"), list)
        or samples[-1] > len(raw["pos"])
    ):
        raise ValueError("Invalid Tracy call-to-signal coordinates or scores")
    if secondary is not None and (
        not isinstance(secondary, str)
        or len(secondary) != len(sequence)
        or not re.fullmatch(r"[ACGTNRYKMSWBDHV]+", secondary.upper())
    ):
        raise ValueError("Invalid Tracy secondary call evidence")
    for base in "ACGT":
        channel = raw.get("peak" + base)
        if (
            not isinstance(channel, list)
            or len(channel) != len(raw["pos"])
            or any(type(value) is not int for value in channel)
        ):
            raise ValueError("Invalid Tracy signal channel")
    return {
        "sequence": sequence.upper(),
        # All arrays retain native event order, even for reverse/trimmed drafts.
        # Absence is unknown evidence, not agreement with the primary call.
        "secondary_sequence": secondary.upper() if secondary is not None else None,
        "positions": [sample - 1 for sample in samples],
        "scores": scores,
        "signal_bounds": list(signal_bounds(raw, signal_threshold)),
    }


def merge(
    paths: list[Path],
    tracy: Path,
    output_dir: Path,
    minimum_overlap: int = 50,
    minimum_identity: float = 0.8,
    signal_threshold: int = 20,
) -> dict:
    if len(paths) not in (1, 2, 3):
        raise ValueError(
            "Exactly 1, 2 or 3 Sanger reads are required; larger assemblies are unsupported"
        )
    if not 1 <= signal_threshold <= 90:
        raise ValueError("Signal cutoff must be between 1 and 90 percent")
    if minimum_overlap < 1 or not 0 < minimum_identity <= 1:
        raise ValueError("Invalid overlap threshold")
    resolved = [path.resolve(strict=True) for path in paths]
    if len(set(resolved)) != len(resolved):
        raise ValueError("Each input must be a distinct AB1 file")
    hashes = []
    for path in resolved:
        with path.open("rb") as source:
            signature = source.read(4)
        # Preserved/exported originals may have content-addressed names. ABIF
        # identity comes from the bytes; a filename suffix is not evidence.
        if signature != b"ABIF":
            raise ValueError(f"Not an ABIF Sanger trace: {path}")
        hashes.append(file_hash(path))
    if len(set(hashes)) != len(hashes):
        raise ValueError("Duplicate trace content is not an independent read")
    executable = shutil.which(str(tracy))
    if executable is None:
        raise ValueError(f"Tracy executable not found: {tracy}")
    tracy = Path(executable).resolve(strict=True)
    if output_dir.exists() and any(output_dir.iterdir()):
        raise ValueError(f"Output directory is not empty: {output_dir}")
    version = subprocess.run(
        [str(tracy), "--version"],
        capture_output=True,
        text=True,
        timeout=20,
        check=False,
    )
    with tempfile.TemporaryDirectory(prefix="basecraft-sanger-consensus-") as temporary:
        work = Path(temporary)
        aliases = []
        for i, source in enumerate(resolved, 1):
            alias = work / f"read_{i}.ab1"
            alias.symlink_to(source)
            aliases.append(alias.name)
        calls = [
            trace_calls(path, tracy, work / f"calls-{i}.json", signal_threshold)
            for i, path in enumerate(resolved)
        ]
        if len(paths) == 1:
            left, right = calls[0]["signal_bounds"]
            consensus = calls[0]["sequence"][left:right]
            report = {
                "status": "draft_requires_review",
                "engine": "Tracy",
                "signal_threshold": signal_threshold,
                "version_output": (version.stdout + version.stderr).strip(),
                "engine_sha256": file_hash(tracy),
                "mode": "basecall",
                "inputs": [
                    {
                        "read": 1,
                        "path": str(resolved[0]),
                        "sha256": hashes[0],
                        "orientation": "forward",
                        "calls": calls[0],
                    }
                ],
                "consensus": consensus,
                "consensus_length": len(consensus),
                "minimum_overlap": minimum_overlap,
                "minimum_identity": minimum_identity,
                **evidence(
                    [("read_1", consensus)],
                    consensus,
                    minimum_overlap,
                    minimum_identity,
                ),
                "limitations": [
                    "Single read: no opposite-read support; Tracy scores are not calibrated probabilities."
                ],
            }
            output_dir.mkdir(parents=True, exist_ok=True)
            (output_dir / "consensus.fa").write_text(f">consensus\n{consensus}\n")
            (output_dir / "reads.align.fa").write_text(f">read_1\n{consensus}\n")
            (output_dir / "evidence.json").write_text(
                json.dumps(report, indent=2) + "\n"
            )
            return report
        mode = "consensus" if len(paths) == 2 else "assemble"
        trimming = ["-t", "0"]
        if len(paths) == 2:
            trimming += [
                "-q",
                str(calls[0]["signal_bounds"][0]),
                "-u",
                str(len(calls[0]["sequence"]) - calls[0]["signal_bounds"][1]),
                "-r",
                str(calls[1]["signal_bounds"][0]),
                "-s",
                str(len(calls[1]["sequence"]) - calls[1]["signal_bounds"][1]),
            ]
        argv = [str(tracy), mode, *trimming, "-o", "result", *aliases]
        run = subprocess.run(
            argv, cwd=work, capture_output=True, text=True, timeout=120, check=False
        )
        if run.returncode:
            raise RuntimeError(
                f"Tracy {mode} failed ({run.returncode}): {run.stderr[-2000:]}"
            )
        aligned = fasta_records(work / "result.align.fa")
        if len(aligned) != len(paths):
            raise ValueError("Tracy silently omitted an input read")
        by_read = {}
        for name, sequence in aligned:
            match = re.match(r"read_(\d+)(?:\s|$)", name)
            if match is None or int(match[1]) in by_read:
                raise ValueError(f"Unexpected or duplicate Tracy alignment row: {name}")
            by_read[int(match[1])] = (name, sequence)
        if set(by_read) != set(range(1, len(paths) + 1)):
            raise ValueError("Tracy output does not include every supplied read")
        aligned = [by_read[i] for i in range(1, len(paths) + 1)]
        consensus_path = work / (
            "result.fa" if mode == "consensus" else "result.cons.fa"
        )
        consensus_records = fasta_records(consensus_path)
        if len(consensus_records) != 1:
            raise ValueError("Tracy did not produce a single consensus")
        consensus = consensus_records[0][1]
        if len(paths) == 3:
            clipped = []
            for i, (name, sequence) in enumerate(aligned):
                native = calls[i]["sequence"]
                lo, hi = calls[i]["signal_bounds"]
                if "(reverse)" in name:
                    native = native.translate(
                        str.maketrans("ACGTNRYKMSWBDHV", "TGCANYRMKSWVHDB")
                    )[::-1]
                    lo, hi = len(native) - hi, len(native) - lo
                ungapped = sequence.replace("-", "")
                # Exact engine-to-engine anchors bound each read's usable span.
                anchors = [
                    (block.a + j, block.b + j)
                    for block in SequenceMatcher(
                        None, ungapped, native, autojunk=False
                    ).get_matching_blocks()
                    for j in range(block.size)
                    if lo <= block.b + j < hi
                ]
                if not anchors:
                    raise ValueError("Cannot locate signal-qualified assembly calls")
                start, end = min(a for a, _ in anchors), max(a for a, _ in anchors)
                event, row = 0, []
                for base in sequence:
                    row.append(base if start <= event <= end else "-")
                    event += base != "-"
                clipped.append((name, "".join(row)))
            covered = [
                i
                for i in range(len(consensus))
                if any(seq[i] != "-" for _, seq in clipped)
            ]
            if not covered:
                raise ValueError("No signal-qualified assembly columns")
            start, end = covered[0], covered[-1] + 1
            consensus = consensus[start:end]
            aligned = [(name, seq[start:end]) for name, seq in clipped]
        proof = evidence(aligned, consensus, minimum_overlap, minimum_identity)
        report = {
            "status": "draft_requires_review",
            "engine": "Tracy",
            "signal_threshold": signal_threshold,
            "version_output": (version.stdout + version.stderr).strip(),
            "engine_sha256": file_hash(tracy),
            "mode": mode,
            "argv": ["tracy", mode, *trimming, "-o", "result", *aliases],
            "inputs": [
                {
                    "read": i,
                    "path": str(path),
                    "sha256": digest,
                    "orientation": "reverse"
                    if "(reverse)" in aligned[i - 1][0]
                    else "forward",
                    "calls": calls[i - 1],
                }
                for i, (path, digest) in enumerate(zip(resolved, hashes), 1)
            ],
            "consensus": consensus,
            "consensus_length": len(consensus.replace("-", "")),
            "minimum_overlap": minimum_overlap,
            "minimum_identity": minimum_identity,
            **proof,
            "limitations": [
                (
                    "Tracy retains terminal calls until an eight-event median falls below "
                    f"{signal_threshold}% of the central median signal; this is not a quality probability."
                ),
                "Tracy basecall events retain original signal-sample coordinates.",
                "Tracy quality scores are not independently calibrated here.",
            ],
        }
        output_dir.mkdir(parents=True, exist_ok=True)
        (output_dir / "consensus.fa").write_text(f">consensus\n{consensus}\n")
        (output_dir / "reads.align.fa").write_text(
            "".join(f">{name}\n{seq}\n" for name, seq in aligned)
        )
        (output_dir / "evidence.json").write_text(json.dumps(report, indent=2) + "\n")
        return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reads", nargs="+", type=Path)
    parser.add_argument("--tracy", type=Path, default=Path("tracy"))
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--minimum-overlap", type=int, default=50)
    parser.add_argument("--minimum-identity", type=float, default=0.8)
    parser.add_argument(
        "--signal-threshold",
        type=int,
        default=20,
        help="Terminal signal cutoff as percent of the central median",
    )
    args = parser.parse_args()
    try:
        result = merge(
            args.reads,
            args.tracy,
            args.output_dir,
            args.minimum_overlap,
            args.minimum_identity,
            args.signal_threshold,
        )
    except (ValueError, RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
        parser.exit(2, f"Sanger consensus refused: {exc}\n")
    print(
        json.dumps(
            {
                key: result[key]
                for key in ("status", "mode", "consensus_length", "conflict_columns")
            }
        )
    )


if __name__ == "__main__":
    main()
