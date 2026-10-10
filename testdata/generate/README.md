# Test data generators

Scripts that build the large synthetic data sets for benchmarks: alignments up to 10,000 × 100,000, a 1 GB FASTA file, a 50,000-tip tree. `just testdata` runs them with a fixed seed into `~/.cache/basecraft-testdata`, outside the synced folder. Small real files go in `../fixtures/` instead.
