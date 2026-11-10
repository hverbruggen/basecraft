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

Integrity and binding examples are exact original-writer captures from DATA-0021
baseline commit 334aaa1, before either literal constructor was replaced. No byte
substitutions were required: setup uses synthetic run IDs, relative result paths
and fixed result bytes. integrity-empty/nonempty.json exercise failed partial
append plus rollback-sync failure using LogWriter's existing fault seam (verified
length zero and one existing journal entry). These markers have no final newline.
binding.json and binding-nested.json come from ArtifactRun::bind for ordinary and
nested Unicode paths; they have one final newline. Runtime oracles compare all
bytes against the original literals at the same inputs without normalization.
Unknown nested extras are injected for typed round-trip tests, not produced by
these fresh writers. The captured nonempty verified length is fixture-specific.

Analysis-run original-writer captures (DATA-0023):
run-referenced.json: reference_input_locked, Unicode anchor filename.
run-null.json: same writer, non-UTF-8 anchor filename represented as null (Unix).
run-generated.json: link_generated_input_locked, prepared document snapshot.
run-saved.json: Session Delimitation Save As, saved status, outputs absent.
run-completed.json / run-completed-empty.json: publish_referenced_analysis,
  outputs present as one public ArtifactRevision / an empty array.
run-prepared.json / run-prepared-no-export.json: runner prepare, exported name /
  operation ID label. No process execution is necessary for these captures.
All eight files are raw actual original-writer bytes from committed baseline
9c3f346, captured before constructor replacement. No normalization, injected
fields, path substitutions or ID changes. Fresh ULIDs remain fixture-specific.
The runtime literal oracles independently reconstruct every field from original
inputs, allocated run identity and expected output bytes (not run/publication
JSON). Round-trip tests additionally exercise unknown status and nested extras;
those mutations are tests, not original writer captures. Representative shapes
only; not exhaustive OS/path/error coverage. Extraction's additional constructor
is inventoried in schemas/README.md; the user subsequently authorized its inclusion.

run-extracted.json: ninth raw original-writer example from Session::extract,
captured at 5b9fc1f, after the other five substitutions. Extraction production
source at that capture is identical to baseline a856d62. The user subsequently
expanded scope to replace this sixth constructor with the same AnalysisRun type.
This separately identified original capture has no normalization; the type
round-trips it too, and the runtime oracle now checks the sixth typed writer.

Alignment-link original-writer captures (DATA-0025):
link-snapshot-inferred.json: create, schema 1, inferred input, parent-relative
  Unicode source, random retained snapshot filename, no trailing newline.
link-explicit-view.json: associate after inferred origin, schema 1, explicit
  presentation association, equal labels do not imply scientific inference.
link-snapshot-extra.json: share_to, schema 1, retained snapshot and nested extra.
link-referenced.json: create_referenced, schema 2, artifact-run reference,
  empty snapshot path, pretty JSON with one trailing newline.
link-referenced-extra.json: share_to, schema 2, same run and nested extra.
All five are raw actual original-writer bytes captured at 9dd2e416b5e2 before
moving AlignmentLink. No normalization, path or ID substitutions, injected
fields or newline changes. Snapshot names and run IDs remain fixture-specific.
The extra cases use the original share writer after an explicit test extension
on the loaded link; they are not claims that fresh create writers merge extras.
Runtime byte oracles use the frozen original wire declaration and fields derived
from source bytes, constructor inputs, returned snapshot paths and allocated run,
not parsed output metadata. Round-trip tests use the protocol type directly.
Rust callers using the ops re-export now also import links::AlignmentLinkExt
for associated functions and methods; public fields/struct literals remain.

Alignment repair/guard original-writer captures (WF-0101):
link-repair-{absent,file,symlink}.json and link-guard-{absent,file,symlink}.json
were captured from unchanged PreparedLinks::publish at d82cead before producer
substitution. repair_record_tests.rs prepares prior filesystem entries and staged
opaque metadata from the existing DS03 captures, then exercises the real publish
writer with an intentionally failing data callback. These are producer captures,
not complete real analysis/Save As journeys. Cases cover absent prior entries,
binary/empty prior files, dangling Unicode symlinks, and intended null/origin/
origin-plus-view. Unix symlink cases are platform-specific. Guard filenames retain
the actual allocated random name. No captured bytes/paths/IDs were substituted.
The original literal oracle uses independent constructor inputs and the allocated
recovery basename, never values decoded from the resulting record. All six files
are pretty recursively sorted JSON without a final newline. Existing behavioral
Save As/retry/manual recovery tests remain separate evidence.
The guard recovery basename is the original Serde OsStr representation: Unix
byte-array object (or Windows UTF-16-unit object on that writer platform), not a
string. Unix guard fixtures have typed OsString round-trip assertions on Unix;
other platforms inspect their exact JSON bytes. This does not add cross-platform
native-filename decoding or change the production selected-field reader.

EX11/EX12/EX13 original publication captures
-----------------------------------------
collection-intent.json, collection-export.json, member-export.json and
member-export-empty.json were captured before typed producer substitution by
runner::record_format_tests::publication_original_producer_bytes. The real
finalize/export_member helpers validated and published test-prepared synthetic
collection files from the existing registry fixture package; no scientific
engine or plugin process ran. Each raw capture was compared byte for byte with
an independent original literal at identical inputs through the existing writer.
Member identity/size/hash came independently from the actual promoted file.
The synthetic absolute /fixture namespace avoids private paths without replacing
bytes. Random directory names, Unicode/native path spelling and actual Unix
filesystem device/inode values are retained, not portable identity guarantees.
All four captures use pretty recursively sorted JSON and one terminal LF.
