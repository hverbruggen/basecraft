# ASAP reference fixture

`mantidactylus.fasta` is the 19-sequence 16S example shipped with ASAP
(iTaxoTools ASAPy `tests/test.fas`, GPL-3.0), with CRLF line endings removed.
Aligned length 478.

The tables come from the reference ASAP command-line source (Achaz and
Brouillet, the copy used on Spartan; core identical to ASAPy 7cd38ba), built on
Linux/glibc with `gcc -O3 -U_FORTIFY_SOURCE`. The only change to that source
writes full-precision copies of existing results (`%.17g`) next to the normal
output files.

- `mantidactylus.reference-jc69.phy`: the reference's own JC69 distances.
- `reference-distances.*.tsv`: `asap -a -x 1 -d 1 in.fas` on the FASTA.
- `basecraft-distances.*.tsv`: `asap -a -x 1 -l 478 in.phy` on the Basecraft
  JC69 matrix of the same FASTA.

`partitions.tsv` lists every candidate partition in final rank order;
`nodes.tsv` gives every agglomeration node's simulated p-value. Native ASAP in
`Stream::Reference` mode reproduces both files exactly with seed 1. Exactness
assumes glibc `log`/`exp`; other C libraries can differ in the last bit.
