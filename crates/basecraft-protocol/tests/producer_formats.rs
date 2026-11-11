use basecraft_protocol::{producer_records::*, sanger_records::*};
use serde::{Serialize, de::DeserializeOwned};
use std::{io, path::Path};

// Python json.dump's compact separators. Pretty producers use two-space indentation.
struct PythonCompact;
impl serde_json::ser::Formatter for PythonCompact {
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
}
fn check<T: DeserializeOwned + Serialize>(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/producers-v1")
        .join(name);
    let original = std::fs::read(path).unwrap();
    let record: T = serde_json::from_slice(&original).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut bytes = Vec::new();
    if original.starts_with(b"{\n") {
        bytes = serde_json::to_vec_pretty(&record).unwrap();
    } else {
        record
            .serialize(&mut serde_json::Serializer::with_formatter(
                &mut bytes,
                PythonCompact,
            ))
            .unwrap();
    }
    if original.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    assert_eq!(bytes, original, "{name}");
}
#[test]
fn bundled_producer_records_round_trip_exactly() {
    check::<SangerDraft>("sanger-draft.json");
    check::<SangerPlacement>("sanger-placement.json");
    check::<MafftAddition>("mafft-addition.json");
    check::<PrimerReport>("primers.json");
    check::<IqtreeRecovery>("iqtree-recovery.json");
    for state in ["ready", "unknown"] {
        check::<ZenodoSelection>(&format!("zenodo-selection-{state}.json"));
        check::<ZenodoReceipt>(&format!("zenodo-receipt-{state}.json"));
    }
}
