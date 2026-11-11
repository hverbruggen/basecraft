"""Predict protein-coding genes with Prodigal, through its Python binding
Pyrodigal, and propose them as CDS features.

Prodigal trains on the sequence itself when it has at least 20 kb
(single mode). Shorter sequences use its preset models (meta mode), which
exist only for genetic codes 4 and 11.
"""

import warnings

import pyrodigal
from basecraft.plugin import Plugin

plugin = Plugin("basecraft.pyrodigal", "0.1.0")


def cds(start, end, strand, partial_start, partial_end, code, label):
    return {
        "op": "feature.add",
        "params": {
            "feature": {
                "kind": "CDS",
                "location": {
                    "segments": [
                        {
                            "start": start,
                            "end": end,
                            "strand": strand,
                            "partial_start": partial_start,
                            "partial_end": partial_end,
                        }
                    ]
                },
                "qualifiers": [
                    {"key": "label", "value": label},
                    {"key": "transl_table", "value": str(code)},
                    {
                        "key": "note",
                        "value": f"predicted by Prodigal {pyrodigal.PRODIGAL_VERSION}",
                    },
                ],
            }
        },
    }


@plugin.operation("predict_genes")
def predict_genes(params, context):
    """Genes on both strands with genetic code `translation_table`,
    optionally only within `start`..`end` (0-based, end exclusive).
    Training uses the whole sequence."""
    code = int(params.get("translation_table", 11))
    if code not in pyrodigal.TRANSLATION_TABLES:
        raise ValueError(f"Prodigal doesn't support genetic code {code}")
    full = bytes(context.snapshot.residues)
    lo = max(0, int(params.get("start", 0)))
    hi = min(len(full), int(params.get("end", len(full))))
    if len(full) >= pyrodigal.MIN_SINGLE_GENOME:
        finder = pyrodigal.GeneFinder(mask=True)
        with warnings.catch_warnings():
            # Pyrodigal recommends 100 kb; organelle genomes train fine from 20 kb.
            warnings.simplefilter("ignore", UserWarning)
            finder.train(full, translation_table=code)
        mode = "single"
    else:
        bins = [
            b
            for b in pyrodigal.METAGENOMIC_BINS
            if b.training_info.translation_table == code
        ]
        if not bins:
            raise ValueError(
                f"Sequences under {pyrodigal.MIN_SINGLE_GENOME // 1000} kb need genetic code 4 or 11"
            )
        finder = pyrodigal.GeneFinder(
            meta=True, metagenomic_bins=pyrodigal.MetagenomicBins(bins), mask=True
        )
        mode = "meta"
    out = []
    for gene in finder.find_genes(full[lo:hi]):
        # Pyrodigal: 1-based inclusive, partial_begin/partial_end at the
        # left and right edges of the sequence.
        strand = "forward" if gene.strand == 1 else "reverse"
        label = f"gene_{len(out) + 1}"
        out.append(
            cds(
                lo + gene.begin - 1,
                lo + gene.end,
                strand,
                gene.partial_begin,
                gene.partial_end,
                code,
                label,
            )
        )
    return {"ops": out, "message": f"{len(out)} genes ({mode} mode, code {code})"}


if __name__ == "__main__":
    plugin.run()
