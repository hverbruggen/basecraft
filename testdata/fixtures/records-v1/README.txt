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

Report-output original writer captures
-------------------------------------
provenance-report-{empty,real}-{manifest,source-map}.json come from the
unchanged provenance::write_reports writer (one terminal LF).
export-report-{empty,real,custom}-{manifest,source-map,citations}.json come
from unchanged Report::export (no appended LF). Empty uses synthetic valid
Publication/DocumentRecord inputs. Real uses FASTA edit/undo/redo plus two
recorded analysis-snapshot activities, without executing a tool; extra output
hashes are test-prepared to expose ordering/duplicates. Custom is an explicit
Report with escaped/native-looking strings, duplicate ids/citations and a
nondefault template. Random publication/activity ids are retained verbatim.
The independent original-literal byte oracles remain in report_record_formats.rs.
Historical provenance-v1 template1/singular-activity files are unchanged and
are not current-writer goldens. These fresh shapes define no reader migration.

Original installation-index capture and runtime serializer controls
-----------------------------------------------------------------
installation-index-{empty,populated,defaulted,mutations-*}.json are raw
bytes from the original registry::write_index, using predeclared synthetic
nonpersonal inputs through its private test module record_format_tests.rs.
The mutation closures simulate state changes; they are not installation,
removal or runtime-provisioning events. Missing-file reading creates no directory.
installation-index-rust-default.json is a direct serializer control (schema 0),
not writer output: missing-file and missing-schema loading instead use schema 1.

managed-plugin-runtime-serializer-{empty-maps,populated}.json use the original
Runtime and the same to_writer_pretty call shape as the runtime.json writer.
These are SERIALIZER-ONLY controls, not real runtime.json producer/provisioning
captures. Paths were supplied before serialization, never redacted/normalized.
They need not exist or pass runtime verification. Windows-looking strings on
Unix demonstrate escaping, not Windows native path behavior.

The independent OldIndex/OldRuntime test serializers retain original struct
field declaration order and construct their inputs independently of production
values/emitted JSON. Index/runtime struct fields retain declaration order,
BTree maps/sets sort, executable Vec order/duplicates survive, and no LF is added.
installation-index-identity-compact.json uses synthetic identity tuples and
Diagnostic values in the registry generation serializer call shape, including
managed_runtime; a separately stated old ordered oracle checks its exact bytes
and hash. This is not discovery or a package/runtime verification capture.

installation-index-reader-observations.json freezes original load_index results
from 157 named cases: exact Error variant/Display/JSON positions and categories,
or decoded semantic values. Input strings are recorded verbatim; large bound
inputs use explicit generation recipes. Observation JSON is test evidence, not
an application-written record. Unknown top fields and duplicate unknown/map
keys keep the final semantic value; duplicate known fields fail. Runtime arrays
of 7/8/9 elements decode (trailing maps default); index arrays fail. Syntactically
bad nested runtime data fails before an unsupported index schema is checked.
Unused semantic runtime problems still decode; no semantic verification runs.

installation-index-writer-observations.json freezes exact output-bound success
and size/closure/load/native-path serialization failures, asserting prior bytes
survive at each reached stage. These are not atomic-replacement failure tests.
The invalid UTF-8 native path case was captured on Unix. Other native platforms,
permission/provisioning semantics and actual generated-path producer evidence
remain separate qualification gates. Reader captures do not set compatibility
policy or make nested unknown fields extension-preserving.

Capture baseline bfe3888a58b2da2d914a6ff2d19ccbf9318681eb retained the original
registry/runtime producer/reader blobs 32e6fad5c1714836890836631bce319dc6933f1a
and 9e5f2c85ba58a5c3ddb073a8a3bb015fa04af54c. Its explicitly accepted atomic IO
dependency blob is ac71087672fe771140d68e6e56a31f93b25defdf; this differs from
the earlier design baseline. Only the private test-module declaration is added
to registry.rs. No production declaration/body or runtime.rs is changed here.
Capture ran as an ordinary user on Linux, offline, with isolated application
profile. No plugin/tool process, discovery, install or provisioning was invoked.

The following original capture lengths/SHA256 freeze bytes before substitution:
installation-index-defaulted.json
  387 bytes; SHA256 8d9f6e7f3fc3536c8342eecd39b2b54b98d3e1632688e74e9b84585d3f9918f3
installation-index-empty.json
  87 bytes; SHA256 2b63e4d598ee05e18549374a2ad0f13deb810e8ff20b87f7dd6980dbe3c84e0b
installation-index-identity-compact.json
  495 bytes; SHA256 f805d77813453cc338a24ccfa475379b86707218071a71dd04422a5f52280ed6
installation-index-mutations-enable.json
  1306 bytes; SHA256 6b8d5b5f91196bd681ceadf430e94841f36081a19ca9add64e895325ea195e43
installation-index-mutations-remove.json
  1293 bytes; SHA256 b660ebe6a9eef5c04b21c686629ff23e5568a3037a56f1b18766a619b752c878
installation-index-mutations-reselect.json
  1724 bytes; SHA256 265485e491f5ede2114ee12d92772c44e826d1bf3a0247116c27f23121f1a3ac
installation-index-populated.json
  1322 bytes; SHA256 19f578f6b4bea55b923cd0be8985816c5cc45e371179e715046dc7e246a643cc
installation-index-reader-observations.json
  69322 bytes; SHA256 f045d90fefde223a04f56846dd5ea41b12820239108c66b2904ec003f50819c3
installation-index-rust-default.json
  87 bytes; SHA256 3cbffd984a404d791169b6e606317848253f77bd3df4008d0457cb57e4b59953
installation-index-writer-observations.json
  931 bytes; SHA256 cf1a3702f45c74785129e81b0f06f7a07ac10eab36ea8249cccb5477f513857d
managed-plugin-runtime-serializer-empty-maps.json
  335 bytes; SHA256 a1935157d720f0f2a65f766f8c96d74d0985157dfa9d24b6e96c4a0949a0a6b3
managed-plugin-runtime-serializer-populated.json
  438 bytes; SHA256 88163aca4682216b5bde247b111e2eae39a8c9acd1e7a11d5f3cc249e896b4b3

RP11 provenance-export-map-{empty,single,shared-ancestry}.json were captured
from the unchanged original input_evidence::export_history writer by its private
export_record_format_tests child module, before any export-map type substitution.
These are raw retained content-addressed artifact bytes, without normalization,
including generated nonpersonal ULIDs, original byte hashes, recursively sorted
Value keys, ordered rows, pretty whitespace and no terminal newline.

Empty is a helper-level control; normal package workflows have a root. Single
archives one synthetic publication. Shared ancestry covers two roots sharing a
parent in one owner and a distinct same-id publication in another canonical
owner; traversal order is the original LIFO postorder. Accepted source records
contain Unicode/escaping and noncanonical raw whitespace, archived unchanged.
Publication currently rejects unknown fields; its rejection is tested rather
than claiming unknown publication extensions were archived successfully.

Fresh writer runs compare full bytes and retained hash/size/path to the original
sorted-Value recipe, with old identities/topology and original byte hashes from
synthetic inputs and generated identities from separately returned relocated
publications/parents. The map under test never supplies oracle identities.
The frozen examples retain one original run's random identities; fresh runs are
not normalized to match those identities and ordinary tests never update them.
Private capture source/input manifests and fault observations accompany the
task handoff. No new production decoder or compatibility policy is implied.
