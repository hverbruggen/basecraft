# basecraft-asap

Native ASAP (Puillandre, Brouillet and Achaz 2021,
doi:10.1111/1755-0998.13281), ported from the reference command-line core by
G. Achaz and S. Brouillet.

`analyse` takes a complete distance matrix in Basecraft's triangular order
`(1,0), (2,0), (2,1), ...` and returns every candidate partition in rank order,
plus the agglomeration tree. In the application, `basecraft-delimitation`
computes p, JC69 or K2P distances with `basecraft-distance` and calls it.

## Random streams

- `Stream::Parallel` is the application default. Each simulation gets its own
  stream, keyed by seed, node and replicate, and runs on the current rayon
  pool. The thread count does not change the result.
- `Stream::Reference` uses glibc `rand()` and glibc's sort tie order. With a
  fixed seed it reproduces the reference program bit for bit on Linux/glibc.
  It runs on one thread.

On 10000 sequences the parallel stream took 2.4 s on 16 threads, against 20 s
for the reference stream. Earlier datasets gave 6.5 to 11 times the
single-thread speed.

## Parallel against reference

Seeds 1 to 50 per stream on two JC69 matrices, build-host, 2026-10-18.

| | Astraptes, 332 seqs, 38 nodes | Halimeda, 583 seqs, 140 nodes |
|---|---|---|
| Median SD of a node p-value across seeds, reference / parallel | 0.0147 / 0.0139 | 0.0154 / 0.0158 |
| SD ratio parallel/reference, median (10th to 90th percentile) | 1.00 (0.82 to 1.14) | 1.01 (0.84 to 1.23) |
| Largest difference in mean p-value between streams | 0.010 | 0.013 |
| Mean difference over nodes | +0.0010 | -0.0001 |
| Best partition over 50 seeds, reference | 24 subsets in all 50 | 30 subsets in 45, 23 in 5 |
| Best partition over 50 seeds, parallel | 24 subsets in all 50 | 30 subsets in 47, 23 in 3 |

Parallel p-values scatter from seed to seed as much as the reference's do.
The largest gap between the two streams' means is under one seed's SD. On
Halimeda the seed decides the best partition in both streams, at about the
same rate. Astraptes shows a +0.001 mean shift, about 7% of one SD. With 50
seeds and correlated nodes I can't tell whether it is real.

## Where this differs from the reference program

- Specimens that are singletons at a zero merge distance stay in the
  partition. The reference drops them. `Analysis::diagnostics` says when.
- Each subset carries its own node's p-value. The reference SPART-XML
  attaches them by position, so they land on the wrong subsets.
- `Settings::max_simulations` is the per-node cap. The reference hard-codes
  100000 and ignores its replicate option.

## Tests and benchmark

`tests/reference.rs` checks the Mantidactylus tables in
`testdata/fixtures/asap/` exactly, on both distance sources (Linux/glibc only).
`examples/asap_bench.rs` is the benchmark driver. The full comparison, data
sources and scripts are in the private workspace notes,
`methods_research/asap_native/README.md`.
