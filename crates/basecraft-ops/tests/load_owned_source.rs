//! Main-file hashes and parsers must describe one owned snapshot.
use basecraft_io::{Source, hash_bytes};
use basecraft_ops::{Session, files};
use std::path::Path;

fn rendered(loaded: &files::Loaded) -> Vec<u8> {
    let mut bytes = Vec::new();
    match loaded {
        files::Loaded::Sequence(opened) => files::write_main(opened, &mut bytes).unwrap(),
        files::Loaded::Alignment(opened) => {
            files::write_alignment(&opened.doc, &mut bytes).unwrap();
        }
        files::Loaded::Tree(opened) => files::write_tree(opened, &mut bytes).unwrap(),
        _ => panic!("unexpected kind"),
    }
    bytes
}

#[test]
fn mutation_after_read_cannot_change_loaded_bytes_or_hash() {
    let dir = tempfile::tempdir().unwrap();
    for (name, original) in [
        ("single.fasta", b">first description\nACGT\n".as_slice()),
        ("ragged.afa", b">a\nACG-\n>b\nAC\n".as_slice()),
        ("forest.nwk", b"(a:1,b:2);\n(c:3,d:4);\n".as_slice()),
        (
            "forest.nex",
            b"#NEXUS\nbegin trees;\ntree one = (a,b);\ntree two = (c,d);\nend;\n".as_slice(),
        ),
        (
            "locus.gb",
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb")
                .as_slice(),
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, original).unwrap();
        let legacy = files::load(&path).unwrap();
        let (source, hash) = Source::open_hashed(&path).unwrap();
        assert_eq!(hash, hash_bytes(original));
        std::fs::write(&path, b"malformed replacement").unwrap();
        let loaded = files::load_from(&path, source).unwrap();
        assert_eq!(
            std::mem::discriminant(&loaded),
            std::mem::discriminant(&legacy)
        );
        assert_eq!(rendered(&loaded), rendered(&legacy), "{name}");
        match (&loaded, &legacy) {
            (files::Loaded::Sequence(a), files::Loaded::Sequence(b)) => {
                assert_eq!(a.format, b.format);
                assert_eq!(a.doc.to_genbank(), b.doc.to_genbank());
            }
            (files::Loaded::Tree(a), files::Loaded::Tree(b)) => {
                assert_eq!(a.nexus, b.nexus);
                assert_eq!(a.remaining.len(), 1);
                assert_eq!(a.remaining.len(), b.remaining.len());
                assert_eq!(a.metadata_warning, b.metadata_warning);
            }
            _ => {}
        }
        assert!(
            files::load(&path).is_err(),
            "replacement must not be parsed"
        );
    }
}

#[test]
fn multirecord_sequence_and_companion_survive_owned_loading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.fasta");
    let bytes = b">a description\nACGT\n>b retained\nTTAA\n";
    std::fs::write(&path, bytes).unwrap();
    std::fs::write(
        files::gff_sidecar(&path),
        b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1;Name=example\n",
    )
    .unwrap();
    let legacy = files::load(&path).unwrap();
    let (source, hash) = Source::open_hashed(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let loaded = files::load_from(&path, source).unwrap();
    assert_eq!(hash, hash_bytes(bytes));
    assert_eq!(rendered(&loaded), rendered(&legacy));
    let files::Loaded::Sequence(opened) = loaded else {
        panic!("GFF companion must select sequence loading");
    };
    assert_eq!(opened.fasta.unwrap().index.records.len(), 2);
    assert_eq!(opened.doc.features().count(), 1);
}

#[test]
fn malformed_empty_missing_and_unsupported_sources_fail() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("bad.fasta", b"not FASTA".as_slice()),
        ("empty.fasta", b"".as_slice()),
        ("bad.gb", b"not GenBank".as_slice()),
        ("empty.gb", b"".as_slice()),
        ("bad.nwk", b"(a,b".as_slice()),
        ("bad-utf8.nwk", b"\xff".as_slice()),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let old = files::load(&path).err().expect("invalid input");
        let new = files::load_from(&path, Source::from_vec(bytes.to_vec()))
            .err()
            .expect("invalid owned input");
        assert_eq!(std::mem::discriminant(&old), std::mem::discriminant(&new));
    }
    assert!(Source::open_hashed(&dir.path().join("missing.fasta")).is_err());
    // An unsupported format must not discard the supplied bytes and reread disk.
    assert!(files::load_from(Path::new("workspace.spart"), Source::from_vec(vec![])).is_err());
}

#[test]
fn session_records_loaded_hash_and_refuses_edits_after_external_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sequence.fasta");
    let original = b">a\nACGT\n";
    std::fs::write(&path, original).unwrap();
    let mut session = Session::new(dir.path());
    let id = session.open(&path).unwrap();
    let record = session.document_record(id).unwrap();
    let open = record
        .events
        .iter()
        .find(|e| matches!(e.op, basecraft_protocol::Op::Open { .. }))
        .unwrap();
    assert_eq!(
        open.result.hashes,
        vec![("sequence.fasta".into(), hash_bytes(original))]
    );
    std::fs::write(&path, b">a\nTTTT\n").unwrap();
    assert!(
        session
            .apply(
                id,
                basecraft_protocol::Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "G".into(),
                }
            )
            .is_err()
    );
    assert_eq!(session.doc(id).read(0, 4), b"ACGT");
}

#[cfg(feature = "test-counters")]
#[test]
fn actual_session_read_counts_separate_kind_probe_and_companion_fallback() {
    use basecraft_io::counters;
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes, expected_session, expected_probe, companion) in [
        ("single.fasta", b">a\nACGT\n".as_slice(), 1, 1, false),
        (
            "alignment.afa",
            b">a\nACGT\n>b\nACGT\n".as_slice(),
            1,
            1,
            false,
        ),
        ("with-gff.fasta", b">a\nACGT\n".as_slice(), 3, 1, true),
        ("tree.nwk", b"(a,b);\n".as_slice(), 1, 0, false),
        (
            "locus.gb",
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb")
                .as_slice(),
            1,
            0,
            false,
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        if companion {
            std::fs::write(files::gff_sidecar(&path), b"##gff-version 3\n").unwrap();
        }
        counters::reset();
        files::kind(&path).unwrap();
        let key = path.canonicalize().unwrap();
        let probe = counters::snapshot()
            .reads
            .get(&key)
            .copied()
            .unwrap_or_default();
        assert_eq!(probe.full_reads, expected_probe);
        counters::reset();
        let mut session = Session::unlogged(dir.path());
        session.open(&path).unwrap();
        let counts = counters::snapshot();
        assert_eq!(counts.unresolved_reads, 0);
        let reads = counts.reads[&key];
        println!(
            "{name}: session={} bytes={} kind_probe={}",
            reads.full_reads, reads.bytes, probe.full_reads
        );
        assert_eq!(reads.full_reads, expected_session);
        assert_eq!(reads.bytes, expected_session * bytes.len() as u64);
        assert_eq!(counts.sync_all + counts.sync_data, 0);
    }
}
