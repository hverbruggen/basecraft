# Draft: owned editable sources and indexed read-only files

Status: draft recommendation; the product's first-paint target is unchanged.

Editable documents currently read one immutable owned buffer and hash the bytes
that their parser consumes. FASTA indexes are rebuilt from those bytes without a disk cache.
Opening runs on a worker and writes no recovery files. The first edit reads
and hashes no source bytes and forces one journal write. No live constructor
used memory mapping; the unused variant and dependency have been removed.

Five isolated warm-cache release samples at `2a05000` opened a 100,000,000-byte
FASTA in 49.057 ms and a 1,000,000,000-byte FASTA in 484.610 ms. A compact durable
edit cost 2.053 ms and a first 4K alignment frame 2.665 ms. These numbers include
core work only, not delivery to the window or display presentation. The shared
host compile lock excluded concurrent compilation. See the dated history and
budget report for all measurements.

Recommendation for editable documents: keep owned buffers. Mapping a file owned
by another program permits an external truncation to fault the process rather
than returning an ordinary read error. Owned buffers cost memory proportional to
the input, but preserve a stable parser and piece-table backing. The present
measurements do not establish that a mapped implementation would be faster.
The 101,666,678-byte FASTA check peaks at 101,672,185 additional requested bytes.
GenBank parses directly from a hashed buffered reader: 118,333,408 input bytes
with 100,000,000 residues peak at 100,066,555 additional bytes. Small-format
measurements and the measured index-rebuild cost are in REPORT.md.
Mapping an application-owned immutable snapshot remains a possible future
optimization requiring its own lifetime and disk-space measurements.

Read-only indexed files are a separate case. A lazy indexed reader can load only
visible ranges and avoid allocating the whole input. Compare bounded positional
reads against mapping an immutable snapshot before selecting its implementation.
Opening a mutable external file with a read-only mapping still has the truncation
risk; “read-only editor” does not make the underlying file immutable. Do not use
editable-document timings to claim constant-memory indexed browsing.

Every editable file uses the same recovery policy. At first edit, a hard link
in application state retains the original inode when both paths are on the same
filesystem; otherwise the journal references the original. Recovery validates
its content. Each edit checks stamps; an external change causes one current-
document checkpoint. Recovery starts from the latest completed save, with
checkpoints for undo across saved history. No size limit or next-to-source
recovery directory remains. An external in-place rewrite followed by a crash
before the next edit can still invalidate reference recovery; the journal is
preserved and recovery refuses to apply it to different bytes.

A decision to change the 1 GB first-paint promise or introduce live mappings still
belongs to the user. This draft does neither.
