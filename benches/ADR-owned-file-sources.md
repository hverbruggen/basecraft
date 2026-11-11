# Draft: owned editable sources and indexed read-only files

Status: draft recommendation; the product's first-paint target is unchanged.

Editable documents currently read one immutable owned buffer and hash the bytes
that their parser consumes. The index cache avoids rebuilding unchanged FASTA
indexes. Opening and recovery preparation run on a worker. The first edit reads
and hashes no source bytes and forces one journal write. No live constructor
used memory mapping; the unused variant and dependency have been removed.

Initial release measurements (three warm-cache samples, neutral machine label
`reference`) opened a 100,000,000-byte FASTA in 74.745 ms. A compact durable edit
cost 2.210 ms and a first 4K alignment frame 2.931 ms. These numbers include core
work only, not delivery to the window or display presentation. See the dated
history and the budget report for memory and 1 GB measurements.

Recommendation for editable documents: keep owned buffers. Mapping a file owned
by another program permits an external truncation to fault the process rather
than returning an ordinary read error. Owned buffers cost memory proportional to
the input, but preserve a stable parser and piece-table backing. The present
measurements do not establish that a mapped implementation would be faster.
The 100 MB requested-heap check peaks at 100,013,801 additional bytes, with
about 14 KB above the raw file size. Small-format decoding and counter overhead
are reported separately in REPORT.md. Mapping only an application-owned immutable snapshot remains a possible future
optimization, but requires its own lifetime and disk-space measurements.

Read-only indexed files are a separate case. A lazy indexed reader can load only
visible ranges and avoid allocating the whole input. Compare bounded positional
reads against mapping an immutable snapshot before selecting its implementation.
Opening a mutable external file with a read-only mapping still has the truncation
risk; “read-only editor” does not make the underlying file immutable. Do not use
editable-document timings to claim constant-memory indexed browsing.

Large editable files use references for crash recovery. Before Basecraft replaces
such an original during Save, it retains the original inode through a hard link
and updates the recovery reference before replacement. This avoids a full byte
copy and keeps earlier undo/replay inputs available. Reference recovery always
verifies content; an external in-place modification can invalidate the reference.
The hard-link mechanism needs filesystem support; failures occur before replacement.

A decision to change the 1 GB first-paint promise or introduce live mappings still
belongs to the user. This draft does neither.
