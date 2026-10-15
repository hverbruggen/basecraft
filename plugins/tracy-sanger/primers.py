"""Bounded, error-tolerant terminal primer search for reviewable masks.

Semiglobal edit alignment permits substitutions and indels, partial primers,
IUPAC primer codes, and either amplicon orientation. N in a read never counts
as matching evidence. These are candidates, not confirmation of primer identity.
"""

IUPAC = dict(
    zip(
        "ACGTNRYKMSWBDHV",
        [
            "A",
            "C",
            "G",
            "T",
            "ACGT",
            "AG",
            "CT",
            "GT",
            "AC",
            "CG",
            "AT",
            "CGT",
            "AGT",
            "ACT",
            "ACG",
        ],
    )
)
COMPLEMENT = str.maketrans("ACGTNRYKMSWBDHV", "TGCANYRMKSWVHDB")


def reverse_complement(sequence):
    return sequence.translate(COMPLEMENT)[::-1]


def terminal_hit(
    sequence, primer, side, minimum_overlap=12, error_percent=25, window=120
):
    if not primer or len(primer) < minimum_overlap:
        return None
    offset = 0 if side == "5" else max(0, len(sequence) - window)
    target = sequence[:window] if side == "5" else sequence[offset:]
    best = None
    for overlap in range(minimum_overlap, len(primer) + 1):
        query = primer[-overlap:] if side == "5" else primer[:overlap]
        # Cells: edit distance, target start, informative matches.
        previous = [(0, j, 0, 0) for j in range(len(target) + 1)]
        for i, a in enumerate(query, 1):
            current = [(i, 0, 0, 0)]
            for j, b in enumerate(target, 1):
                match = b in "ACGT" and b in IUPAC[a]
                informative = match and a != "N"
                bits = (
                    (2 if len(IUPAC[a]) == 1 else 1 if len(IUPAC[a]) == 2 else 0)
                    if match
                    else 0
                )
                diagonal = previous[j - 1]
                delete = previous[j]
                insert = current[j - 1]
                options = [
                    (
                        diagonal[0] + (not match),
                        diagonal[1],
                        diagonal[2] + informative,
                        diagonal[3] + bits,
                    ),
                    (delete[0] + 1, delete[1], delete[2], delete[3]),
                    (insert[0] + 1, insert[1], insert[2], insert[3]),
                ]
                current.append(min(options, key=lambda v: (v[0], -v[3], -v[2], v[1])))
            previous = current
        for end, (errors, start, informative, bits) in enumerate(previous):
            if (
                errors * 100 > overlap * error_percent
                or end - start < minimum_overlap
                or bits - 3 * errors < 20
            ):
                continue
            hit = {
                "side": side,
                "start": start + offset,
                "end": end + offset,
                "overlap": overlap,
                "errors": errors,
                "informative_matches": informative,
                "evidence_score": bits - 3 * errors,
            }
            rank = (
                bits - 3 * errors,
                overlap,
                -errors,
                -(start if side == "5" else len(target) - end),
            )
            if best is None or rank > best[0]:
                best = rank, hit
    return None if best is None else best[1]


def detect(
    sequence, forward, reverse, minimum_overlap=12, error_percent=25, window=120
):
    sequence = sequence.upper().replace("-", "")
    forward, reverse = (
        "".join(forward.upper().split()),
        "".join(reverse.upper().split()),
    )
    if not sequence or len(sequence) > 100_000 or set(sequence) - IUPAC.keys():
        raise ValueError("Expected an IUPAC DNA consensus")
    if any(len(p) > 200 or set(p) - IUPAC.keys() for p in [forward, reverse]):
        raise ValueError(
            "Primers must contain at most 200 IUPAC DNA bases, entered 5′ to 3′"
        )
    if (
        not 8 <= minimum_overlap <= 100
        or not 0 <= error_percent <= 40
        or not 20 <= window <= 500
    ):
        raise ValueError("Invalid primer search thresholds")
    alternatives = []
    for orientation, left, right in [
        ("forward", forward, reverse_complement(reverse)),
        ("reverse", reverse, reverse_complement(forward)),
    ]:
        hits = [
            h
            for h in [
                terminal_hit(
                    sequence, left, "5", minimum_overlap, error_percent, window
                ),
                terminal_hit(
                    sequence, right, "3", minimum_overlap, error_percent, window
                ),
            ]
            if h
        ]
        score = sum(h["evidence_score"] for h in hits)
        alternatives.append((score, orientation, hits))
    _, orientation, hits = max(alternatives, key=lambda v: v[0])
    start = max([h["end"] for h in hits if h["side"] == "5"] or [0])
    end = min([h["start"] for h in hits if h["side"] == "3"] or [len(sequence)])
    if start >= end:
        raise ValueError(
            "Primer candidates would mask the entire consensus; adjust the search"
        )
    return {
        "keep_start": start,
        "keep_end": end,
        "orientation": orientation,
        "hits": hits,
        "forward": forward,
        "reverse": reverse,
        "minimum_overlap": minimum_overlap,
        "error_percent": error_percent,
        "window": window,
    }
