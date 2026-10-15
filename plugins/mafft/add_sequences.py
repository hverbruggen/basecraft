"""Lossless MAFFT --add with explicit detection of keeplength truncation."""

import json
import subprocess
import tempfile
from pathlib import Path

from basecraft.tools import allocated_cpus, read_alignment, write_atomic


def run_add(
    source, additions, output, mafft, *, orientation=False, keep=True, threads=None
):
    reference = read_alignment(source, minimum=1)
    names = [name for name, _ in additions]
    if not additions or len(names) != len(set(names)):
        raise ValueError("New sequence identifiers must be unique")
    with tempfile.TemporaryDirectory(prefix="basecraft-mafft-add-") as directory:
        work = Path(directory)
        original = work / "reference.fa"
        probe = work / "new.fa"
        original.write_text(
            "".join(f">ref{i}\n{seq}\n" for i, (_, seq) in enumerate(reference))
        )
        probe.write_text(
            "".join(
                f">new{i}\n{seq.replace('-', '').replace('.', '')}\n"
                for i, (_, seq) in enumerate(additions)
            )
        )
        # Bound parallel work so a small addition does not occupy every CPU.
        command = [
            mafft,
            "--preservecase",
            "--thread",
            str(allocated_cpus(threads)),
        ]
        if orientation:
            # MAFFT recommends the faster 6-mer method for ordinary related
            # sequences; its DP orientation mode is for highly divergent data.
            command.append("--adjustdirection")
        command += ["--add", str(probe)]
        if keep:
            command.append("--mapout")
        command.append(str(original))
        result = subprocess.run(
            command, cwd=work, capture_output=True, text=True, timeout=600, check=False
        )
        if result.returncode:
            raise RuntimeError(f"MAFFT addition failed: {result.stderr[-2000:]}")
        aligned = work / "result.fa"
        aligned.write_text(result.stdout)
        records = read_alignment(aligned, minimum=1)
        rows = dict(records)
        if len(rows) != len(reference) + len(additions):
            raise ValueError("MAFFT changed the number of rows")
        refs = []
        for i, (_, seq) in enumerate(reference):
            found = rows.get(f"ref{i}")
            if found is None or found.replace("-", "").replace(".", "") != seq.replace(
                "-", ""
            ).replace(".", ""):
                raise ValueError(
                    "MAFFT changed a reference sequence or its orientation"
                )
            refs.append(found)
        new = []
        reversed_names = []
        for i, (name, seq) in enumerate(additions):
            key = f"new{i}"
            reverse = f"_R_{key}" in rows
            found = rows.get(f"_R_{key}" if reverse else key)
            if found is None:
                raise ValueError("MAFFT omitted a new sequence")
            if reverse:
                reversed_names.append(name)
            new.append((name, found))
        discarded = 0
        if keep:
            mapping = Path(str(probe) + ".map")
            if not mapping.exists():
                raise ValueError("MAFFT did not write its position map")
            discarded = sum(
                line.strip().split(",")[-1].strip() == "-"
                for line in mapping.read_text().splitlines()
                if line.strip() and not line.startswith((">", "#"))
            )
            if refs != [seq for _, seq in reference]:
                raise ValueError("MAFFT changed the frozen reference alignment")
        else:
            # MAFFT removes original all-gap columns by default. Restore those
            # columns while validating the unchanged original column pattern.
            all_rows = refs + [seq for _, seq in new]
            original_columns = list(zip(*(seq for _, seq in reference), strict=True))
            result_columns = list(zip(*all_rows, strict=True))
            restored = []
            old = 0
            for col in result_columns:
                projected = col[: len(refs)]
                if any(base not in "-." for base in projected):
                    while old < len(original_columns) and all(
                        b in "-." for b in original_columns[old]
                    ):
                        restored.append(original_columns[old] + ("-",) * len(new))
                        old += 1
                    if (
                        old >= len(original_columns)
                        or projected != original_columns[old]
                    ):
                        raise ValueError(
                            "MAFFT changed the reference alignment column pattern"
                        )
                    old += 1
                restored.append(col)
            while old < len(original_columns):
                if any(b not in "-." for b in original_columns[old]):
                    raise ValueError("MAFFT omitted a reference column")
                restored.append(original_columns[old] + ("-",) * len(new))
                old += 1
            all_rows = [
                "".join(col[i] for col in restored) for i in range(len(all_rows))
            ]
            refs = all_rows[: len(refs)]
            new = [(name, all_rows[len(refs) + i]) for i, (name, _) in enumerate(new)]
        if discarded == 0:
            complement = str.maketrans(
                "ACGTURYKMSWBDHVNacgturykmswbdhvn", "TGCAAYRMKSWVHDBNtgcaayrmkswvhdbn"
            )
            for (name, before), (_, after) in zip(additions, new, strict=True):
                expected = before.replace("-", "").replace(".", "")
                if name in reversed_names:
                    expected = expected.translate(complement)[::-1]
                if after.replace("-", "").replace(".", "") != expected:
                    raise ValueError("MAFFT lost or changed a new sequence base")
        payload = {
            "rows": new,
            "reference": refs,
            "insertions": discarded,
            "reversed": reversed_names,
            "parameters": command[1:-1],
        }
        write_atomic(output, json.dumps(payload) + "\n")
        return payload
