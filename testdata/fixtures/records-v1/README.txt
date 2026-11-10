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
