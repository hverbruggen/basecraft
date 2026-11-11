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
type Encoder = fn(&[u8], bool, bool) -> Vec<u8>;
fn encode<T: DeserializeOwned + Serialize>(original: &[u8], python: bool, sorted: bool) -> Vec<u8> {
    let record: T = serde_json::from_slice(original).unwrap();
    fn render(record: &impl Serialize, original: &[u8], python: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if original.starts_with(b"{\n") || original.starts_with(b"[\n") {
            bytes = serde_json::to_vec_pretty(record).unwrap();
        } else if python {
            record
                .serialize(&mut serde_json::Serializer::with_formatter(
                    &mut bytes,
                    PythonCompact,
                ))
                .unwrap();
        } else {
            bytes = serde_json::to_vec(record).unwrap();
        }
        if original.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        bytes
    }
    if sorted {
        render(&serde_json::to_value(record).unwrap(), original, python)
    } else {
        render(&record, original, python)
    }
}
#[test]
fn bundled_producer_records_round_trip_exactly() {
    use basecraft_protocol::{distance_records::*, query_records::Query};
    let formats: &[(&str, Encoder, bool, bool)] = &[
        (
            "sanderson-report",
            encode::<basecraft_protocol::sanderson_records::SandersonReport>,
            true,
            true,
        ),
        ("sanger-draft", encode::<SangerDraft>, true, false),
        ("sanger-placement", encode::<SangerPlacement>, true, false),
        ("mafft-addition", encode::<MafftAddition>, true, false),
        ("primers", encode::<PrimerReport>, true, false),
        ("iqtree-recovery", encode::<IqtreeRecovery>, true, false),
        ("zenodo-selection-", encode::<ZenodoSelection>, true, false),
        ("zenodo-receipt-", encode::<ZenodoReceipt>, true, false),
        ("blast-", encode::<BlastReceipt>, true, true),
        ("mptp-failure", encode::<MptpFailure>, true, true),
        ("query-source", encode::<Vec<Query>>, false, false),
        ("haplotypes", encode::<Haplotypes>, false, false),
        (
            "delimitation-progress",
            encode::<DelimitationProgress>,
            false,
            false,
        ),
        (
            "delimitation-report",
            encode::<DelimitationReport>,
            true,
            false,
        ),
        (
            "distance-identified",
            encode::<IdentifiedMatrix>,
            false,
            false,
        ),
        (
            "distance-groups",
            encode::<DistanceGroupReport>,
            false,
            false,
        ),
        ("distance-matrix", encode::<DistanceMatrix>, false, false),
        ("distance-cache-header", encode::<CacheHeader>, false, false),
        ("distance-report", encode::<DistanceRunReport>, false, true),
        ("bootstrap", encode::<BootstrapReport>, false, false),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/producers-v1");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        let (_, encoder, python, sorted) = formats
            .iter()
            .find(|(prefix, ..)| name.starts_with(prefix))
            .unwrap_or_else(|| panic!("no protocol type registered for {name}"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            String::from_utf8(encoder(&bytes, *python, *sorted)).unwrap(),
            String::from_utf8(bytes).unwrap(),
            "{name}"
        );
    }
}
