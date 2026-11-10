Captured from the original Session::track / Session::save_checked writers before
their typed replacement. record_formats.rs drives a recoverable FASTA edit, a
publication failure after replacement, and Save As of a new GenBank sequence.
The test compares complete production bytes against the original literal builders,
using independently read journal entries and Session::save_hashes_at inputs.

These portable examples retain the captured shapes, ordering, hashes, explicit
null, omitted baseline result fields, whitespace and lack of trailing newline.
Only fixture setup values were replaced: temporary root/target paths became
project-relative synthetic paths; session/open ULIDs, timestamp and machine name
became synthetic labels. The script actor was selected in test setup. No source
hash or expected hash was replaced. Production-path comparisons normalize nothing.

All JSON records use the existing version 1. These fixtures are recovery state,
not a promise about portable recovery roots or long-term format compatibility.

Store examples were captured from the original ArtifactRun::create and
input_evidence::register paths by store_registry_formats.rs before replacement.
store.json is compact plus newline; store-registry-*.json are pretty plus newline.
store-extended.json is an injected readable marker with unknown fields, and the
empty registry is a test-written example of the existing missing-file default
(no production writer emits an empty registry). Other registry files are actual
first/update writer outputs, including a nested extension. Only the temporary
root was replaced with /synthetic/records-v1; no real paths or hosts are fixtures.
These paths demonstrate LOCAL state shape, not portable project references.

Execution and local-index examples were captured from the ORIGINAL production
writers before constructor substitutions (DATA-0018 baseline4). Process receipts
cover success with and without tool/execution evidence and invocation failure.
Workflow receipts cover failure before stages and success through synthetic
registered stages; no scientific engines/services are used. These exercise the
real receipt writers via a test-prepared workflow context, not a real analysis.
The nine index states are written by persist() using synthetic OperationOutcomes,
including explicit null ended/diagnostics and a first-error diagnostic. They are
not nine actual job lifecycle transitions; runtime.json remains authoritative.

Only unpredictable/private runtime paths, run/ledger IDs, interpreter, process
instance and timestamps were replaced by synthetic values. Keys, values outside
those substitutions, nulls/omissions, hashes, nested order, spacing and final
newline are preserved. Raw original captures and the exact substitution map stay
private. In-process full-byte oracles run the retained original literal builders
through the existing atomic JSON writer at the SAME runtime values, comparing
all bytes to the current production file. stdout/stderr and terminal ended time
are taken from that file; their keys/null rules/time shape are checked, while
these oracles do not independently verify their contents or wall-clock accuracy.
