# Compact biological examples and robustness fixtures

`just testdata` now exports this compact tier without compiling. Output defaults
 to `~/.cache/basecraft-testdata/compact`; an existing directory is refused, so
manual edits cannot be overwritten. Use a fresh destination for another export:

```sh
python3 testdata/compact/generate.py --out ~/.cache/basecraft-testdata/my-demo
python3 testdata/compact/generate.py --check
python3 -m unittest discover -s testdata/compact -v
```

`just testdata small` and `just testdata full` retain the existing Rust generator
and sizes. They are opt-in stress tiers (small includes a 10 MB FASTA), and must
run on the designated remote compilation host under its shared lock. Ordinary
Rust tests read `testdata/fixtures/compact` directly and never skip for a missing
cache. Python construction/hash/budget checks run in `just check` and `just ci`.
All generated compact data together is below 100 KB (a stricter checked limit
than the 10 MB task budget). No tools, downloads or scientific inference are
needed to generate it. There is deliberately no large compact stress option.

## Construction, provenance and limits

All new bytes are original synthetic examples, distributed under the project's
GPL-3.0-or-later license. No user data, empirical sequences, accessions or
redistributed third-party datasets are included. Seed **1001** ranks internal
codon positions by SHA-256 of `1001:<position>`; this is fixed and independent of
Python PRNG versions. `MANIFEST.json` records every payload's origin category,
expectation, size and SHA-256. The manifest does not hash itself. The generator
version is 2; a change to construction requires reviewing and regenerating the
committed bytes and expectations together.

The 180 nt ancestor is a designed 60-codon template including ATG and TAA. It
represents a short coding segment for editing tests, not a natural gene or a
claim of protein function/folding. Eight descendants share three internal
branches, each with eight inherited synonymous changes; four leaves have one
additional synonymous change. The last three descendants share a one-codon
in-frame deletion at zero-based codon 40. Thus substitutions are correlated by
ancestry rather than independent random strings. Specimens 01/02 and 07/08 are
exact repeats; six haplotypes have multiplicities 2,1,1,1,1,2. These construction
groups are **not validated species** or guaranteed outputs of any method.

The 421 nt locus has a forward CDS joining 1..61 and 91..209, a 29 nt designed
GT..AG intron, a 32 nt spacer and a reverse CDS at 242..421. Both CDS translate
to the ancestor's 59 amino acids. The splice junction splits a codon; GFF phases
are 0 and 2, independently checked. This small locus omits real regulatory and
expression constraints. The protein and spacer FASTA files are separate input
molecule examples. GenBank/GFF coordinates and translation expectations are
checked independently of application parsing code.

The time trees have stipulated arbitrary time units and root-to-tip length 1
(tolerance 1e-12). `time-binary.nwk` has distinct positive internal event times;
`time.nwk` deliberately has ties/polytomies. The substitution tree perturbs specimen_03 on the binary topology (0.08 to
0.18), retains eight matching IDs and deliberately violates ultrametricity and has stipulated substitutions/site. These hand-built
trees are workflow inputs, not calibrated estimates of the codon simulation.
Newick itself does not identify branch units. `hypotheses.spart` includes a
partial assignment. Deduplication is literal aligned identity after uppercase
and U-to-T normalization: gaps/ambiguity are not wildcard matches. Do not infer
species equivalence from repeated sequences. The tiny analytical distance
example has exact uncorrected distances 1/12, 1/12 and 2/12 (tolerance 1e-12).
For stochastic tools use seed 1001 where supported and verify taxon sets and
finite branches, not an asserted universal topology or species count.

The AB1 examples implement a minimal processed ABIF schema: 36 calls, 444 samples,
GATC channel order, PLOC type 4 (signed short), PCON type 2 (char),
triangular peaks, quality 8 at ends/ambiguous calls and 35
elsewhere. Reverse calls are reverse-complemented; a disagreement and N are
intentional. These are **format and display fixtures**, not an instrument noise
model, empirical Sanger validation, or a guarantee of Tracy compatibility.
Independent directory decoding asserts canonical tag types, widths, counts and
signed peak/quality values without using the generator or application parser.
Absent qualities remain unknown. Invalid peak offsets and truncated payloads
are kept separately under `recovery-errors/invalid`.

## Primary format references

References consulted 2026-10-15; no sequence data copied from them:

- [INSDC feature table, maintained by DDBJ](https://www.ddbj.nig.ac.jp/ddbj/feature-table-e.html):
  location, strand and CDS conventions.
- [Sequence Ontology GFF3 specification](https://github.com/The-Sequence-Ontology/Specifications/blob/master/gff3.md):
  one-based inclusive coordinates and CDS phase.
- [Applied Biosystems ABIF specification, July 2006 (archived manufacturer PDF)](https://archive.gfjc.fiu.edu/workshops/resources/literature/ABIF_File_Format.pdf):
  big-endian directory entries, inline payloads and processed signal tags;
  printed pages 14 and 52–53 define PLOC short and PCON char.
- [mPTP input specification](https://github.com/Pas-Kapli/mptp/wiki/Input-data):
  rooted binary tree input. The compact substitution tree is checked through
  `substitution_tips(..., true)`; method validation does not claim an empirical fit.

See [COVERAGE.md](COVERAGE.md) for consumers, known permissive readers and gaps.
The six demo folders contain README.txt procedures. Failure injection belongs
in temporary test directories; never damage existing user provenance for demos.
