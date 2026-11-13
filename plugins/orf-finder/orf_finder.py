"""Find open reading frames on both strands and propose them as features.

Vectorised with numpy over the shared-memory residues: no copy of the
sequence is made, apart from the reverse complement.
"""

import numpy as np
from basecraft.genetic_code import stop_codons
from basecraft.plugin import Plugin, feature

plugin = Plugin("basecraft.orf-finder", "0.1.0")

# A, C, G, T -> 0..3; anything else -> 4, which never forms a codon.
CODE = np.full(256, 4, dtype=np.uint8)
for i, b in enumerate(b"ACGT"):
    CODE[b] = i
    CODE[b + 32] = i
COMPLEMENT = np.arange(256, dtype=np.uint8)
for a, b in zip(b"ACGTacgt", b"TGCAtgca"):
    COMPLEMENT[a] = b
START = 0 * 16 + 3 * 4 + 2  # ATG


def codon_index(codon: str) -> int:
    """`codon` in this module's A, C, G, T numbering."""
    a, b, c = (int(CODE[ord(base)]) for base in codon)
    return a * 16 + b * 4 + c


def orfs_on(seq: np.ndarray, min_codons: int, stops: tuple[int, ...]):
    """(start, end) of ORFs on the forward strand of `seq`, end exclusive:
    ATG to the next in-frame codon of `stops`."""
    codes = CODE[seq]
    n = len(codes) - 2
    if n <= 0:
        return
    codon = codes[:n].astype(np.int16) * 16 + codes[1 : n + 1] * 4 + codes[2 : n + 2]
    invalid = (codes[:n] > 3) | (codes[1 : n + 1] > 3) | (codes[2 : n + 2] > 3)
    codon[invalid] = -1
    for frame in range(3):
        c = codon[frame::3]
        stops_at = np.flatnonzero(np.isin(c, stops))
        starts = np.flatnonzero(c == START)
        if len(stops_at) == 0 or len(starts) == 0:
            continue
        # For each stop, the first ATG after the previous stop.
        prev = np.concatenate(([-1], stops_at[:-1]))
        i = np.searchsorted(starts, prev + 1)
        found = i < len(starts)
        first = starts[np.minimum(i, len(starts) - 1)]
        hit = found & (first < stops_at) & (stops_at - first >= min_codons)
        for start, stop in zip(first[hit], stops_at[hit], strict=True):
            yield frame + 3 * int(start), frame + 3 * int(stop) + 3


@plugin.operation("find_orfs")
def find_orfs(params, context):
    """ORFs of at least `min_codons` codons on both strands, from ATG to a
    stop codon of `genetic_code`, optionally only within `start`..`end`
    (0-based, end exclusive). The code has no default."""
    if params.get("genetic_code") is None:
        raise ValueError("Finding ORFs needs a genetic code: its stop codons end each ORF")
    code = int(params["genetic_code"])
    stops = tuple(codon_index(codon) for codon in stop_codons(code))
    min_codons = int(params.get("min_codons", 100))
    full = context.snapshot.residues
    lo = max(0, int(params.get("start", 0)))
    hi = min(len(full), int(params.get("end", len(full))))
    seq = full[lo:hi]
    length = len(seq)
    out = []
    for s, e in orfs_on(seq, min_codons, stops):
        out.append(feature("misc_feature", lo + s, lo + e, "+", note="ORF", label=f"orf_{len(out) + 1}"))
    rc = COMPLEMENT[seq[::-1]]
    for s, e in orfs_on(rc, min_codons, stops):
        out.append(feature("misc_feature", lo + length - e, lo + length - s, "-", note="ORF", label=f"orf_{len(out) + 1}"))
    return {"ops": out, "message": f"{len(out)} ORFs of at least {min_codons} codons, genetic code {code}"}


@plugin.operation("selftest")
def selftest(params, context):
    """Reports whether the residues arrived without a copy."""
    seq = context.snapshot.residues
    zero_copy = not seq.flags.owndata and not seq.flags.writeable
    return {"ops": [], "message": f"length={len(seq)} zero_copy={zero_copy}"}


if __name__ == "__main__":
    plugin.run()
