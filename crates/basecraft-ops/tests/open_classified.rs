//! Classification belongs to the payload admitted by the common open path.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;

use basecraft_io::hash_bytes;
use basecraft_ops::{DocId, Session, files, metadata, submission};
use basecraft_protocol::Op;

fn payload_kind(payload: &files::Loaded) -> files::Kind {
    match payload {
        files::Loaded::SangerReview(_) => files::Kind::SangerReview,
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
    assert_eq!(payload_kind(&files::load(path).unwrap()), expected);
    let root = path.parent().unwrap();
    let mut plain = Session::unlogged(root);
    let old = plain.open(path).unwrap();
    let mut classified = Session::unlogged(root);
    let (id, kind) = classified.open_classified(path).unwrap();
    assert_eq!(kind, expected);
    assert_eq!(classified.state(id).unwrap(), plain.state(old).unwrap());
    match kind {
        files::Kind::SangerReview => assert!(classified.sanger_review(id).is_ok()),
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
    crate::isolated_profile::isolate!();
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
                if let Ok(loaded) = files::load(&path) {
                    agree(&path, payload_kind(&loaded));
                } else {
                    let root = path.parent().unwrap();
                    let old = Session::unlogged(root).open(&path).unwrap_err();
                    let new = Session::unlogged(root).open_classified(&path).unwrap_err();
                    assert_eq!(std::mem::discriminant(&old), std::mem::discriminant(&new));
                }
                *count += 1;
            }
        }
    }
    crate::isolated_profile::isolate!();
    let mut count = 0;
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures"),
        &mut count,
    );
    assert!(count >= 23);
}

#[test]
fn explicit_alignment_names_keep_precedence_over_gff() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
            std::fs::write(&path, b">a\nTTTTT\n").unwrap();
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
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
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
        (
            "plain.nwk",
            std::fs::read(fixtures.join("tree-inference/unrooted.nwk")).unwrap(),
            false,
        ),
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

#[test]
fn classification_validates_through_the_last_record() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("late.fasta");
    let mut bytes = b">a\nA\n>b\nT\n>later\n".to_vec();
    bytes.extend(std::iter::repeat_n(
        b'A',
        usize::try_from(files::LONGEST_ALIGNMENT_ROW).unwrap() + 1,
    ));
    std::fs::write(&path, &bytes).unwrap();
    agree(&path, files::Kind::Sequence);
    bytes.extend_from_slice(b"\n>invalid\nA C\n");
    std::fs::write(&path, bytes).unwrap();
    assert!(
        Session::unlogged(dir.path())
            .open_classified(&path)
            .is_err()
    );
}

#[test]
fn bound_tree_errors_warn_and_preserve_the_standard_tree() {
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bound.nwk");
    std::fs::write(&path, b"(a,b);\n").unwrap();
    let run = ArtifactRun::create(&path, "bound-tree").unwrap();
    run.bind(&path).unwrap();
    // A valid adjacent sidecar cannot rescue a mismatched bound main file.
    let tree = basecraft_docs::TreeDoc::from_newick("bound", "(a,b);").unwrap();
    let mut sidecar = Vec::new();
    let hash = basecraft_io::hash_bytes(b"(a,b);\n");
    files::write_companion(&files::tree_companion(&tree, &hash), &mut sidecar).unwrap();
    std::fs::write(dir.path().join("bound.nwk.basecraft.json"), sidecar).unwrap();
    std::fs::write(&path, b"(other,tree);\n").unwrap();
    let mut session = Session::unlogged(dir.path());
    let (id, _) = session.open_classified(&path).unwrap();
    assert!(session.tree_metadata_warning(id).unwrap().is_some());
    assert_eq!(session.tree(id).unwrap().to_newick(), "(other,tree);");
    std::fs::write(&path, b"(a,b);\n").unwrap();
    let (id, kind) = session.open_classified(&path).unwrap();
    assert_eq!((id, kind), (DocId(1), files::Kind::Tree));
    assert!(session.tree_metadata_warning(id).unwrap().is_some());
}

#[cfg(feature = "test-counters")]
#[test]
fn companion_capture_counts_every_read_on_both_session_routes() {
    use basecraft_io::counters;
    crate::isolated_profile::isolate!();
    let main = b">a description\nACGT\n>b retained\nTTAA\n";
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1\n";
    for name in ["records.fasta", "records.afa", "records.aln.fasta"] {
        for classified in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            let sidecar = files::gff_sidecar(&path);
            std::fs::write(&path, main).unwrap();
            std::fs::write(&sidecar, gff).unwrap();
            let mut session = Session::new(dir.path());
            counters::reset();
            let id = if classified {
                session.open_classified(&path).unwrap().0
            } else {
                session.open(&path).unwrap()
            };
            let counts = counters::snapshot();
            for (file, bytes) in [(&path, main.as_slice()), (&sidecar, gff.as_slice())] {
                assert_eq!(
                    counts.reads[&file.canonicalize().unwrap()],
                    counters::Reads {
                        full_reads: 1,
                        bytes: bytes.len() as u64,
                    }
                );
            }
            assert_eq!(counts.unresolved_reads, 0);
            assert_eq!((counts.sync_data, counts.sync_all), (0, 0));
            assert!(!dir.path().join(".basecraft").exists());
            let record = session.document_record(id).unwrap();
            let open = record
                .events
                .iter()
                .find(|e| matches!(e.op, Op::Open { .. }))
                .unwrap();
            let mut expected = vec![(name.into(), hash_bytes(main))];
            if name == "records.fasta" {
                expected.push(("records.gff3".into(), hash_bytes(gff)));
            }
            assert_eq!(open.result.hashes, expected);
            println!(
                "{name} classified={classified}: main=1/{} GFF=1/{} unresolved=0 sync=0",
                main.len(),
                gff.len()
            );
        }
    }
}

#[test]
fn gff_capture_preserves_error_and_alignment_contracts() {
    use basecraft_ops::Error;
    crate::isolated_profile::isolate!();
    for name in ["records.fasta", "records.afa", "records.aln.fasta"] {
        for gff in [
            b"".as_slice(),
            b"##gff-version 3\nother\t.\tgene\t1\t2\t.\t+\t.\tID=x\n",
            b"bad GFF\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            let sidecar = files::gff_sidecar(&path);
            std::fs::write(&path, b">a\nACGT\n>b\nTTAA\n").unwrap();
            std::fs::write(&sidecar, gff).unwrap();
            if gff == b"bad GFF\n" && name == "records.fasta" {
                for error in [
                    files::load(&path).err().unwrap(),
                    files::open(&path).err().unwrap(),
                    Session::new(dir.path()).open(&path).unwrap_err(),
                ] {
                    assert!(matches!(
                        error,
                        Error::Format(basecraft_io::Error::Parse { .. })
                    ));
                }
            } else {
                agree(
                    &path,
                    if name == "records.fasta" {
                        files::Kind::Sequence
                    } else {
                        files::Kind::Alignment
                    },
                );
            }
            std::fs::remove_file(&sidecar).unwrap();
            std::fs::create_dir(&sidecar).unwrap();
            assert!(matches!(files::open(&path).err().unwrap(), Error::Io(_)));
            assert!(matches!(
                Session::new(dir.path()).open(&path).unwrap_err(),
                Error::Format(basecraft_io::Error::Io(_))
            ));
        }
    }
}

#[test]
fn invalid_main_with_present_gff_keeps_empty_and_parse_errors() {
    use basecraft_ops::Error;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.fasta");
    std::fs::write(files::gff_sidecar(&path), b"##gff-version 3\n").unwrap();
    for bytes in [
        b"".as_slice(),
        b"not FASTA",
        b">a\nAC\n>b\nGG\n>late\nA C\n",
    ] {
        std::fs::write(&path, bytes).unwrap();
        for error in [
            files::load(&path).err().unwrap(),
            Session::new(dir.path()).open(&path).unwrap_err(),
        ] {
            if bytes.is_empty() {
                assert!(matches!(error, Error::Empty(_)));
            } else {
                assert!(matches!(
                    error,
                    Error::Format(basecraft_io::Error::Parse { .. })
                ));
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn unreadable_gff_is_not_absent_and_direct_errors_remain_io() {
    use basecraft_ops::Error;
    use std::os::unix::fs::PermissionsExt;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("denied.fasta");
    let sidecar = files::gff_sidecar(&path);
    std::fs::write(&path, b">a\nACGT\n").unwrap();
    std::fs::write(&sidecar, b"##gff-version 3\n").unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o0)).unwrap();
    // This test intentionally requires an ordinary user, as does validation.
    assert_eq!(
        std::fs::read(&sidecar).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    for error in [
        files::load(&path).err().unwrap(),
        files::open(&path).err().unwrap(),
    ] {
        assert!(
            matches!(error, Error::Io(e) | Error::Format(basecraft_io::Error::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied)
        );
    }
    assert!(
        matches!(Session::new(dir.path()).open(&path).unwrap_err(), Error::Format(basecraft_io::Error::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied)
    );
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o600)).unwrap();
}
