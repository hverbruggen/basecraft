//! Main-file hashes and parsers must describe one owned snapshot.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    assert!(matches!(&legacy, files::Loaded::Sequence(_)));
    let files::Loaded::Sequence(opened) = loaded else {
        panic!("GFF companion must select sequence loading");
    };
    assert_eq!(opened.fasta.unwrap().index.records.len(), 2);
    assert_eq!(opened.doc.features().count(), 1);
}

#[test]
fn malformed_empty_missing_and_unsupported_sources_fail() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
            .apply(id, basecraft_protocol::Op::seq_replace(0, 1, "G"))
            .is_err()
    );
    assert_eq!(session.doc(id).unwrap().read(0, 4), b"ACGT");
}

#[test]
fn companion_hash_remains_guarded_on_the_captured_path() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sequence.fasta");
    let sidecar = files::gff_sidecar(&path);
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1\n";
    std::fs::write(&path, b">a\nACGT\n").unwrap();
    std::fs::write(&sidecar, gff).unwrap();
    let mut session = Session::new(dir.path());
    let id = session.open(&path).unwrap();
    assert_eq!(session.doc(id).unwrap().features().count(), 1);
    let record = session.document_record(id).unwrap();
    let open = record
        .events
        .iter()
        .find(|e| matches!(e.op, basecraft_protocol::Op::Open { .. }))
        .unwrap();
    assert!(
        open.result
            .hashes
            .contains(&("sequence.gff3".into(), hash_bytes(gff)))
    );
    std::fs::write(sidecar, b"##gff-version 3\n").unwrap();
    assert!(
        session
            .apply(id, basecraft_protocol::Op::seq_replace(0, 1, "G"))
            .is_err()
    );
    assert_eq!(session.doc(id).unwrap().read(0, 4), b"ACGT");
    assert_eq!(session.doc(id).unwrap().features().count(), 1);
}

#[cfg(feature = "test-counters")]
#[test]
fn actual_session_read_counts_separate_kind_probe_and_companion_capture() {
    use basecraft_io::counters;
    crate::isolated_profile::isolate!();
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
        ("with-gff.fasta", b">a\nACGT\n".as_slice(), 1, 1, true),
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

#[test]
fn missing_inputs_report_filesystem_errors() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    for name in ["missing.fa", "missing.gb", "missing.nwk"] {
        assert!(matches!(
            files::load(&dir.path().join(name)),
            Err(basecraft_ops::Error::Io(_)
                | basecraft_ops::Error::Format(basecraft_io::Error::Io(_)))
        ));
    }
}

#[test]
fn bound_tree_verifies_owned_snapshot_after_main_mutation_or_removal() {
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let original = b"(a:1,b:2);\n(c:3,d:4);\n";
    for metadata in [false, true] {
        for mutation in ["unchanged", "write", "replace", "remove"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("bound.nwk");
            std::fs::write(&path, original).unwrap();
            let run = ArtifactRun::create(&path, "bound-tree").unwrap();
            run.bind(&path).unwrap();
            let expected = files::load_tree(&path).unwrap();
            if metadata {
                let mut bytes = Vec::new();
                files::tree_companion(&expected.doc, &hash_bytes(original))
                    .write(&mut bytes)
                    .unwrap();
                std::fs::write(run.artifact("bound.nwk.basecraft.json").unwrap(), bytes).unwrap();
            }
            let (source, hash) = Source::open_hashed(&path).unwrap();
            assert_eq!(hash, hash_bytes(original));
            #[cfg(feature = "test-counters")]
            let key = path.canonicalize().unwrap();
            match mutation {
                "write" => std::fs::write(&path, b"(changed,tree);\n").unwrap(),
                "replace" => {
                    basecraft_io::write_atomic(&path, |w| w.write_all(b"(new,tree);\n")).unwrap();
                }
                "remove" => std::fs::remove_file(&path).unwrap(),
                _ => {}
            }
            #[cfg(feature = "test-counters")]
            basecraft_io::counters::reset();
            let files::Loaded::Tree(loaded) = files::load_from(&path, source).unwrap() else {
                panic!("expected tree");
            };
            assert_eq!(loaded.doc.to_newick(), expected.doc.to_newick());
            assert_eq!(loaded.remaining.len(), 1);
            assert_eq!(loaded.remaining[0], expected.remaining[0]);
            assert_eq!(loaded.metadata_warning.is_some(), !metadata);
            #[cfg(feature = "test-counters")]
            {
                let counts = basecraft_io::counters::snapshot();
                assert_eq!(counts.unresolved_reads, 0);
                assert_eq!(
                    counts.reads.get(&key).map_or(0, |r| r.full_reads),
                    0,
                    "bound load_from reread main: metadata={metadata} mutation={mutation}"
                );
            }
            // The disk verifier must still reject changed/missing disk contents.
            if mutation != "unchanged" {
                assert!(ArtifactRun::for_result_checked(&path).is_err());
            }
        }
    }
}

#[cfg(feature = "test-counters")]
#[test]
fn bound_tree_session_with_missing_metadata_reads_main_once() {
    use basecraft_io::counters;
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bound.nwk");
    let bytes = b"(a,b);\n";
    std::fs::write(&path, bytes).unwrap();
    let run = ArtifactRun::create(&path, "bound-tree").unwrap();
    run.bind(&path).unwrap();
    let key = path.canonicalize().unwrap();
    counters::reset();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&path).unwrap();
    assert!(session.tree_metadata_warning(doc).unwrap().is_some());
    let counts = counters::snapshot();
    assert_eq!(counts.unresolved_reads, 0);
    assert_eq!(
        counts.reads[&key],
        counters::Reads {
            full_reads: 1,
            bytes: bytes.len() as u64
        }
    );
    println!(
        "bound tree, missing metadata: Session main reads=1 bytes={}",
        bytes.len()
    );
}

#[test]
fn bound_tree_snapshot_verification_keeps_binding_run_namespace_and_metadata_guards() {
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let original = b"(a,b);\n";
    for fault in [
        "snapshot",
        "binding-schema",
        "binding-result",
        "binding-run",
        "missing-run",
        "namespace",
        "metadata-version",
        "metadata-exchange",
        "metadata-tree",
        "metadata-legacy",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bound.nwk");
        std::fs::write(&path, original).unwrap();
        let run = ArtifactRun::create(&path, "bound-tree").unwrap();
        run.bind(&path).unwrap();
        let source = Source::open(&path).unwrap();
        let source = if fault == "snapshot" {
            Source::from_vec(b"(other,tree);\n".to_vec())
        } else {
            source
        };
        let store = dir.path().join(".basecraft/analyses-v1");
        let binding = std::fs::read_dir(store.join("bindings"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        match fault {
            "binding-schema" | "binding-result" | "binding-run" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&binding).unwrap()).unwrap();
                match fault {
                    "binding-schema" => value["schema_version"] = 2.into(),
                    "binding-result" => value["result"] = "other.nwk".into(),
                    _ => value["run"] = "../outside".into(),
                }
                std::fs::write(binding, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "missing-run" => std::fs::remove_dir_all(run.directory()).unwrap(),
            "namespace" => std::fs::write(
                store.join("store.json"),
                b"{\"format\":\"foreign\",\"schema_version\":1}",
            )
            .unwrap(),
            "metadata-version" | "metadata-exchange" | "metadata-tree" | "metadata-legacy" => {
                let opened = files::load_tree(&path).unwrap();
                let mut bytes = Vec::new();
                files::tree_companion(&opened.doc, &hash_bytes(original))
                    .write(&mut bytes)
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                match fault {
                    "metadata-version" => value["version"] = 2.into(),
                    "metadata-legacy" => value["version"] = 0.into(),
                    "metadata-exchange" => {
                        value["exchange_hash"] = hash_bytes(b"other bytes").into();
                    }
                    _ => value["tree"]["tree_hash"] = hash_bytes(b"other tree").into(),
                }
                std::fs::write(
                    run.artifact("bound.nwk.basecraft.json").unwrap(),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }
            _ => {}
        }
        // Faulty or mismatched bindings leave the given bytes open without metadata.
        let files::Loaded::Tree(tree) = files::load_from(&path, source).unwrap() else {
            panic!("expected tree");
        };
        let warning = tree.metadata_warning.unwrap_or_default();
        assert!(
            warning.starts_with("Tree metadata was ignored"),
            "{fault}: {warning}"
        );
        assert!(tree.doc.supports().is_empty(), "{fault}");
        let expected = if fault == "snapshot" {
            "(other,tree);"
        } else {
            "(a,b);"
        };
        assert_eq!(tree.doc.to_newick().trim(), expected, "{fault}");
    }
}

#[test]
fn captured_companion_refusals_preserve_baseline_undo_and_journal() {
    use basecraft_protocol::Op;
    crate::isolated_profile::isolate!();
    if !isolated_capture_test("captured_companion_refusals_preserve_baseline_undo_and_journal") {
        return;
    }
    for change_main in [false, true] {
        for save in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("records.fasta");
            let sidecar = files::gff_sidecar(&path);
            let main = b">a description\nACGT\n";
            let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1\n";
            std::fs::write(&path, main).unwrap();
            std::fs::write(&sidecar, gff).unwrap();
            let mut session = Session::new(dir.path());
            let id = session.open(&path).unwrap();
            let before = session.document_record(id).unwrap();
            let target = if change_main { &path } else { &sidecar };
            let mut replacement = std::fs::read(target).unwrap();
            if change_main {
                replacement[15] = b'T';
            } else {
                replacement[16] = b'b';
            }
            replacement.push(b'\n');
            std::fs::write(target, replacement).unwrap();
            let result = if save {
                session.save(id, Some(&dir.path().join("copy.fasta")))
            } else {
                session.apply(id, Op::seq_replace(0, 1, "G")).map(|_| ())
            };
            assert!(matches!(
                result,
                Err(basecraft_ops::Error::ChangedOnDisk { .. })
            ));
            assert_eq!(session.doc(id).unwrap().read(0, 4), b"ACGT");
            assert_eq!(session.doc(id).unwrap().features().count(), 1);
            assert!(!session.can_undo(id).unwrap());
            assert!(!session.can_redo(id).unwrap());
            assert_eq!(
                serde_json::to_value(session.document_record(id).unwrap()).unwrap(),
                serde_json::to_value(before).unwrap()
            );
            assert_eq!(
                basecraft_ops::log::read_all(dir.path()).unwrap(),
                Vec::new()
            );
            assert!(!dir.path().join("recovery.json").exists());
            assert!(!dir.path().join("copy.fasta").exists());
        }
    }
}

#[test]
fn captured_multirecord_writer_and_replay_inputs_match_legacy() {
    crate::isolated_profile::isolate!();
    if !isolated_capture_test("captured_multirecord_writer_and_replay_inputs_match_legacy") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.fasta");
    let sidecar = files::gff_sidecar(&path);
    let main = b">a description\nACGT\n>b retained\nTTAA\n";
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1;Name=example\n";
    std::fs::write(&path, main).unwrap();
    std::fs::write(&sidecar, gff).unwrap();
    let legacy = files::open(&path).unwrap();
    let mut session = Session::new(dir.path());
    let id = session.open(&path).unwrap();
    assert_eq!(
        session.doc(id).unwrap().to_genbank(),
        legacy.doc.to_genbank()
    );
    session
        .apply(id, basecraft_protocol::Op::seq_replace(0, 1, "G"))
        .unwrap();
    session.undo(id).unwrap();
    assert_eq!(
        session.doc(id).unwrap().to_genbank(),
        legacy.doc.to_genbank()
    );
    session.redo(id).unwrap();
    let report = basecraft_ops::records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{report:#?}");
}

fn isolated_capture_test(name: &str) -> bool {
    if std::env::var("BASECRAFT_CAPTURE_TEST_CHILD").as_deref() == Ok(name) {
        return true;
    }
    let isolated = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("BASECRAFT_CAPTURE_TEST_CHILD", name)
        .env("BASECRAFT_PROFILE_DIR", isolated.path().join("profile"))
        .status()
        .unwrap();
    assert!(status.success(), "isolated capture test failed: {name}");
    false
}

#[test]
fn cold_recoverable_edit_reads_no_source_bytes_and_recovery_needs_the_originals() {
    crate::isolated_profile::isolate!();
    if !isolated_capture_test(
        "cold_recoverable_edit_reads_no_source_bytes_and_recovery_needs_the_originals",
    ) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("séquences.fasta");
    let sidecar = files::gff_sidecar(&path);
    let main = ">a café\nACGT\n".as_bytes();
    let gff = b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=gene1;Name=example\n";
    std::fs::write(&path, main).unwrap();
    std::fs::write(&sidecar, gff).unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let (id, _) = session.open_classified(&path).unwrap();
    let recovery = session.recovery_root().to_path_buf();
    assert!(!recovery.exists());
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::reset();
    session
        .apply(id, basecraft_protocol::Op::seq_replace(0, 1, "G"))
        .unwrap();
    #[cfg(feature = "test-counters")]
    {
        let counts = basecraft_io::counters::snapshot();
        assert_eq!(counts.unresolved_reads, 0);
        assert!(
            counts.reads.is_empty(),
            "first edit must read no source bytes"
        );
    }
    let expected = session.doc(id).unwrap().to_genbank();
    drop(session);
    let destination = dir.path().join("recovered");
    basecraft_ops::records::recover(&recovery, &destination, None).unwrap();
    let restored = files::open(&destination.join("séquences.fasta")).unwrap();
    assert_eq!(restored.doc.to_genbank(), expected);
    assert_eq!(restored.fasta.unwrap().index.records.len(), 1);
    // Without the unchanged original nothing is recovered; the journal keeps the edit.
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&sidecar).unwrap();
    let missing = dir.path().join("missing");
    assert!(
        basecraft_ops::records::recover(&recovery, &missing, None)
            .map_or(true, |outputs| outputs.is_empty())
    );
}
