"""Find open reading frames on both strands and propose them as features.

Vectorised with numpy over the shared-memory residues: no copy of the
sequence is made, apart from the reverse complement.
"""

import numpy as np
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
STOPS = (3 * 16 + 0 * 4 + 0, 3 * 16 + 0 * 4 + 2, 3 * 16 + 2 * 4 + 0)  # TAA TAG TGA
START = 0 * 16 + 3 * 4 + 2  # ATG


def orfs_on(seq: np.ndarray, min_codons: int):
    """(start, end) of ORFs on the forward strand of `seq`, end exclusive."""
    codes = CODE[seq]
    n = len(codes) - 2
    if n <= 0:
        return
    codon = codes[:n].astype(np.int16) * 16 + codes[1 : n + 1] * 4 + codes[2 : n + 2]
    invalid = (codes[:n] > 3) | (codes[1 : n + 1] > 3) | (codes[2 : n + 2] > 3)
    codon[invalid] = -1
    for frame in range(3):
        c = codon[frame::3]
        stops = np.flatnonzero(np.isin(c, STOPS))
        starts = np.flatnonzero(c == START)
        if len(stops) == 0 or len(starts) == 0:
            continue
        # For each stop, the first ATG after the previous stop.
        prev = np.concatenate(([-1], stops[:-1]))
        i = np.searchsorted(starts, prev + 1)
        found = i < len(starts)
        first = starts[np.minimum(i, len(starts) - 1)]
        hit = found & (first < stops) & (stops - first >= min_codons)
        for start, stop in zip(first[hit], stops[hit], strict=True):
            yield frame + 3 * int(start), frame + 3 * int(stop) + 3


@plugin.operation("find_orfs")
def find_orfs(snapshot, params):
    min_codons = int(params.get("min_codons", 100))
    seq = snapshot.residues
    length = len(seq)
    out = []
    for s, e in orfs_on(seq, min_codons):
        out.append(feature("misc_feature", s, e, "+", note="ORF", label=f"orf_{len(out) + 1}"))
    rc = COMPLEMENT[seq[::-1]]
    for s, e in orfs_on(rc, min_codons):
        out.append(feature("misc_feature", length - e, length - s, "-", note="ORF", label=f"orf_{len(out) + 1}"))
    return {"ops": out, "message": f"{len(out)} ORFs of at least {min_codons} codons"}


@plugin.operation("selftest")
def selftest(snapshot, params):
    """Reports whether the residues arrived without a copy."""
    seq = snapshot.residues
    zero_copy = not seq.flags.owndata and not seq.flags.writeable
    return {"ops": [], "message": f"length={len(seq)} zero_copy={zero_copy}"}


@plugin.operation("noop")
def noop(snapshot, params):
    return []


if __name__ == "__main__":
    plugin.run()
