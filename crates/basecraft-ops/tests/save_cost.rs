//! Public save cost and retained-history policy, measured in isolated processes.
#![cfg(all(feature = "test-counters", feature = "test-heap"))]

#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::Session;
use basecraft_protocol::Op;
use std::io::Write;

#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

#[test]
fn save_streams_small_and_large_documents_once() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    for (extension, kind) in [
        ("fasta", "sequence"),
        ("afa", "alignment"),
        ("gb", "sequence"),
    ] {
        for size in [
            4096,
            if extension == "gb" {
                78_000_000
            } else {
                100_000_000
            },
        ] {
            let path = root.join(format!("input-{size}.{extension}"));
            let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            if extension == "gb" {
                writeln!(
                    file,
                    "LOCUS       input {size} bp DNA linear UNK 01-JAN-1970"
                )
                .unwrap();
                file.write_all(b"FEATURES             Location/Qualifiers\nORIGIN\n")
                    .unwrap();
                for i in 0..size / 60 {
                    writeln!(
                        file,
                        "{:>9} aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa",
                        i * 60 + 1
                    )
                    .unwrap();
                }
                file.write_all(b"//\n").unwrap();
            } else {
                file.write_all(b">input\n").unwrap();
                for _ in 0..size / 60 {
                    file.write_all(&[b'A'; 60]).unwrap();
                    file.write_all(b"\n").unwrap();
                }
            }
            file.flush().unwrap();
            drop(file);
            let length = std::fs::metadata(&path).unwrap().len();
            let mut session = Session::new(&root);
            let id = session.open(&path).unwrap();
            let edit = if kind == "sequence" {
                Op::seq_replace(0, 1, "T")
            } else {
                Op::AlnReplace {
                    row: 0,
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                }
            };
            session.apply(id, edit).unwrap();
            basecraft_io::counters::reset();
            let heap = basecraft_io::heap::Measurement::start().unwrap();
            session.save(id, None).unwrap();
            let heap = heap.finish().unwrap();
            let counts = basecraft_io::counters::snapshot();
            assert_eq!(counts.reads[&path].full_reads, 1, "{counts:?}");
            assert_eq!(counts.reads[&path].bytes, length);
            assert_eq!(counts.serializations[kind], 1);
            assert!(
                heap.incremental_peak_bytes <= 8 << 20,
                "{kind}: {} extra heap bytes",
                heap.incremental_peak_bytes
            );
            let bundle = basecraft_ops::records::companion(&path);
            assert_eq!(
                std::fs::read_dir(bundle.join("artifacts")).unwrap().count(),
                0
            );
            let record: basecraft_protocol::provenance::Publication =
                serde_json::from_slice(&std::fs::read(bundle.join("manifest.json")).unwrap())
                    .unwrap();
            assert!(
                record
                    .outputs
                    .iter()
                    .all(|output| output.retained.is_none())
            );
            assert!(
                record
                    .events
                    .iter()
                    .any(|event| matches!(event.op, Op::Save { .. }))
            );
            eprintln!(
                "{extension} {length} bytes: one source read, one serialization, {} extra heap bytes",
                heap.incremental_peak_bytes
            );
        }
    }
}
