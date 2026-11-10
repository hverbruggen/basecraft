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
