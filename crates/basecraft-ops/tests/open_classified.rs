//! Classification belongs to the payload admitted by the common open path.
use std::path::Path;

use basecraft_io::hash_bytes;
use basecraft_ops::{DocId, Session, files, metadata, submission};
use basecraft_protocol::Op;

fn payload_kind(payload: files::Loaded) -> files::Kind {
    match payload {
        files::Loaded::Sequence(_) => files::Kind::Sequence,
        files::Loaded::Alignment(_) => files::Kind::Alignment,
        files::Loaded::Tree(_) => files::Kind::Tree,
        files::Loaded::Delimitation(_) => files::Kind::Delimitation,
        files::Loaded::Metadata(_) => files::Kind::Metadata,
        files::Loaded::Submission(_) => files::Kind::Submission,
    }
}

fn agree(path: &Path, expected: files::Kind) {
    assert_eq!(files::kind(path).unwrap(), expected);
    assert_eq!(payload_kind(files::load(path).unwrap()), expected);
    let root = path.parent().unwrap();
    let mut plain = Session::unlogged(root);
    let old = plain.open(path).unwrap();
    let mut classified = Session::unlogged(root);
    let (id, kind) = classified.open_classified(path).unwrap();
    assert_eq!(kind, expected);
    assert_eq!(classified.state(id), plain.state(old));
    match kind {
        files::Kind::Sequence => assert!(classified.checked_sequence(id).is_ok()),
        files::Kind::Alignment => assert!(classified.alignment(id).is_ok()),
        files::Kind::Tree => assert!(classified.tree(id).is_ok()),
        files::Kind::Delimitation => assert!(classified.delimitation(id).is_ok()),
        files::Kind::Metadata => assert!(classified.metadata(id).is_ok()),
        files::Kind::Submission => assert!(classified.submission(id).is_ok()),
    }
}

#[test]
fn all_six_payloads_agree_across_open_apis() {
    use files::Kind;
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes, kind) in [
        ("one.fasta", b">a\nACGT\n".as_slice(), Kind::Sequence),
        (
            "rows.afa",
            b">a\nACGT\n>b\nAC--\n".as_slice(),
            Kind::Alignment,
        ),
        ("forest.nwk", b"(a,b);\n(c,d);\n".as_slice(), Kind::Tree),
        (
            "locus.gb",
            include_bytes!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb")
                .as_slice(),
            Kind::Sequence,
        ),
        (
            "hypotheses.spart",
            include_bytes!(
                "../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
            )
            .as_slice(),
            Kind::Delimitation,
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        agree(&path, kind);
    }
    agree(&metadata::create(dir.path()).unwrap(), Kind::Metadata);
    let path = dir.path().join("review.submission.json");
    submission::create(
        &mut Session::unlogged(dir.path()),
        &path,
        &dir.path().join("rows.afa"),
    )
    .unwrap();
    agree(&path, Kind::Submission);
}

#[test]
fn all_fasta_fixtures_return_the_loaded_kind() {
    fn visit(dir: &Path, count: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, count);
            } else if matches!(files::Format::from_path(&path), Ok(files::Format::Fasta)) {
                // Fixtures are opened passively; no copy, edit or durable action.
                agree(&path, files::kind(&path).unwrap());
                *count += 1;
            }
        }
    }
    let mut count = 0;
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures"),
        &mut count,
    );
    assert!(count >= 23);
}

#[test]
fn explicit_alignment_names_keep_precedence_over_gff() {
    for name in ["rows.fasta", "rows.afa", "rows.aln.fasta"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, b">a\nACGT\n>b\nACGT\n").unwrap();
        std::fs::write(files::gff_sidecar(&path), b"##gff-version 3\n").unwrap();
        agree(
            &path,
            if name == "rows.fasta" {
                files::Kind::Sequence
            } else {
                files::Kind::Alignment
            },
        );
    }
}

#[test]
fn failed_preparation_admits_no_document_and_preserves_errors() {
    let dir = tempfile::tempdir().unwrap();
    let valid = dir.path().join("valid.fasta");
    std::fs::write(&valid, b">a\nACGT\n").unwrap();
    let mut session = Session::unlogged(dir.path());
    for (name, bytes) in [
        ("empty.fasta", b"".as_slice()),
        ("late.fasta", b">a\nAC\n>b\nGG\n>later\nA C\n".as_slice()),
        ("bad.nwk", b"(a,b".as_slice()),
        ("bad.gb", b"not GenBank".as_slice()),
        ("unknown.txt", b"data".as_slice()),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let old = Session::unlogged(dir.path()).open(&path).unwrap_err();
        let new = session.open_classified(&path).unwrap_err();
        assert_eq!(std::mem::discriminant(&old), std::mem::discriminant(&new));
    }
    assert_eq!(
        files::kind(&dir.path().join("empty.fasta")).unwrap(),
        files::Kind::Sequence
    );
    let missing = dir.path().join("missing.fasta");
    assert!(session.open_classified(&missing).is_err());
    let (id, _) = session.open_classified(&valid).unwrap();
    assert_eq!(id, DocId(0));
    assert!(session.alignment(id).is_err());
    assert!(session.checked_sequence(DocId(999)).is_err());
}

#[test]
fn loaded_hash_and_first_durable_action_guards_are_preserved() {
    for companion in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("one.fasta");
        let bytes = b">a\nACGT\n";
        std::fs::write(&path, bytes).unwrap();
        let side = files::gff_sidecar(&path);
        if companion {
            std::fs::write(&side, b"##gff-version 3\n").unwrap();
        }
        let mut session = Session::new(dir.path());
        let (id, kind) = session.open_classified(&path).unwrap();
        assert_eq!(kind, files::Kind::Sequence);
        let record = session.document_record(id).unwrap();
        assert!(record.events.iter().any(|e| {
            matches!(e.op, Op::Open { .. })
                && e.result
                    .hashes
                    .contains(&("one.fasta".into(), hash_bytes(bytes)))
        }));
        if companion {
            std::fs::write(&side, b"##gff-version 3\n# changed\n").unwrap();
        } else {
            let modified = path.metadata().unwrap().modified().unwrap();
            std::fs::write(&path, b">a\nTTTT\n").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
        assert!(
            session
                .apply(
                    id,
                    Op::SeqReplace {
                        start: 0,
                        end: 1,
                        residues: "G".into()
                    }
                )
                .is_err()
        );
        assert_eq!(session.doc(id).unwrap().read(0, 4), b"ACGT");
    }
}

#[cfg(feature = "test-counters")]
#[test]
fn classified_route_reads_main_once_without_passive_sync() {
    use basecraft_io::counters;
    use basecraft_ops::analysis_store::ArtifactRun;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
    for (name, bytes, bound) in [
        (
            "alignment.afa",
            std::fs::read(fixtures.join("alignment-editing/ambiguous-gappy.afa")).unwrap(),
            false,
        ),
        (
            "locus.gb",
            std::fs::read(fixtures.join("sequences-annotations/locus.gb")).unwrap(),
            false,
        ),
        ("plain.nwk", b"(a:1,b:2);\n(c:3,d:4);\n".to_vec(), false),
        ("bound.nwk", b"(a,b);\n".to_vec(), true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, &bytes).unwrap();
        if bound {
            ArtifactRun::create(&path, "bound-tree")
                .unwrap()
                .bind(&path)
                .unwrap();
        }
        let key = path.canonicalize().unwrap();
        counters::reset();
        let mut session = Session::unlogged(dir.path());
        let (id, _) = session.open_classified(&path).unwrap();
        let counts = counters::snapshot();
        assert_eq!(counts.unresolved_reads, 0);
        assert_eq!(
            counts.reads[&key],
            counters::Reads {
                full_reads: 1,
                bytes: bytes.len() as u64
            }
        );
        assert_eq!((counts.sync_data, counts.sync_all), (0, 0));
        if bound {
            assert!(session.tree_metadata_warning(id).unwrap().is_some());
        }
        println!("{name}: classified main reads=1 bytes={}", bytes.len());
    }
}
