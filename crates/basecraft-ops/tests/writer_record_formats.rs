use basecraft_ops::{files, metadata, submission, writer_versions};
use std::path::Path;

fn golden(format: &str, variant: &str, path: &Path, bytes: Vec<u8>) {
    let version = writer_versions::for_path(path)[format];
    let name = format!("{format}-{variant}-v{version}.bin");
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/writers")
        .join(name);
    if std::env::var_os("BASECRAFT_CAPTURE_WRITERS").is_some() {
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    } else {
        assert_eq!(
            bytes,
            std::fs::read(&target).expect("a new writer version needs its own golden"),
            "{} changed without a writer version bump",
            target.display()
        );
    }
}

#[test]
fn document_writer_bytes_require_a_version_bump() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/compact");
    for (file, format) in [
        ("sequences-annotations/locus.fasta", "fasta"),
        ("sequences-annotations/locus.gb", "genbank"),
    ] {
        let path = root.join(file);
        let files::Loaded::Sequence(opened) = files::load(&path).unwrap() else {
            panic!("sequence fixture")
        };
        let mut bytes = Vec::new();
        files::write_main(&opened, &mut bytes).unwrap();
        golden(format, "sequence", &path, bytes);
        let mut bytes = Vec::new();
        if let Some(result) = files::write_sidecar(&opened, &mut bytes) {
            result.unwrap();
            golden("gff3", "sequence", &path, bytes);
        }
    }
    let path = root.join("alignment-editing/homologues.afa");
    let files::Loaded::Alignment(opened) = files::load(&path).unwrap() else {
        panic!("alignment fixture")
    };
    let mut bytes = Vec::new();
    files::write_alignment(&opened.doc, &mut bytes).unwrap();
    golden("fasta", "alignment", &path, bytes);
    let path = root.join("tree-inference/unrooted.nwk");
    let opened = files::load_tree(&path).unwrap();
    let mut bytes = Vec::new();
    files::write_tree(&opened, &mut bytes).unwrap();
    let hash = basecraft_io::hash_bytes(&bytes);
    golden("newick", "tree", &path, bytes);
    let mut bytes = Vec::new();
    files::write_tree_sidecar_at(&opened.doc, &hash, &mut bytes).unwrap();
    golden("tree-sidecar", "tree", &path, bytes);
    let path = root.join("species-delimitation/hypotheses.spart");
    let workspace = files::load_delimitation(&path).unwrap();
    for xml in [false, true] {
        let mut bytes = Vec::new();
        files::write_delimitation(&workspace, xml, &mut bytes).unwrap();
        golden("spart", if xml { "xml" } else { "text" }, &path, bytes);
    }
    let mut bytes = Vec::new();
    files::write_delimitation_sidecar(&workspace, "b3:fixture", &mut bytes).unwrap();
    golden("delimitation-sidecar", "legacy", &path, bytes);
    #[cfg(unix)]
    {
        let mut bytes = Vec::new();
        files::write_delimitation_sidecar_at(
            &workspace,
            "b3:fixture",
            Path::new("/fixture.spart"),
            &mut bytes,
        )
        .unwrap();
        golden("delimitation-sidecar", "current", &path, bytes);
    }
    {
        use basecraft_ops::links::{AlignmentLink, AlignmentLinkExt};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.afa");
        let tree = root.path().join("tree.nwk");
        std::fs::write(&source, ">a\nAC\n>b\nAT\n").unwrap();
        std::fs::write(&tree, "(a,b);\n").unwrap();
        AlignmentLink::create(&tree, &source, &source, 7, true, None).unwrap();
        let bytes = std::fs::read_to_string(basecraft_ops::links::sidecar(&tree)).unwrap();
        let link: AlignmentLink = serde_json::from_str(&bytes).unwrap();
        // Allocation chooses a random unused snapshot name. Freeze only that
        // name; retain the writer's ordering, whitespace and all other bytes.
        let allocated = serde_json::to_string(&link.snapshot).unwrap();
        golden(
            "alignment-link",
            "snapshot",
            &tree,
            bytes.replace(&allocated, "\"snapshot.afa\"").into_bytes(),
        );
    }
    for (i, bytes) in metadata::encode(&Default::default())
        .unwrap()
        .into_iter()
        .enumerate()
    {
        golden(
            if i == 0 {
                "metadata-manifest"
            } else {
                "metadata-tsv"
            },
            &i.to_string(),
            Path::new("metadata.basecraft.json"),
            bytes,
        );
    }
    golden(
        "submission",
        "default",
        Path::new("sample.submission.json"),
        submission::encode(&Default::default()).unwrap(),
    );
}

#[test]
fn replay_checks_matching_writers_and_keeps_fingerprint_checks_for_other_versions() {
    use basecraft_ops::{Session, log, replay};
    use basecraft_protocol::Op;
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path());
    let doc = session.create_sequence("sample", "ACGT").unwrap();
    session
        .save(doc, Some(&root.path().join("sample.gb")))
        .unwrap();
    drop(session);
    let entries = log::read_all(root.path()).unwrap();
    let save = entries
        .iter()
        .find(|e| matches!(e.op, Op::Save { .. }))
        .unwrap();
    let Op::Save { writer, .. } = &save.op else {
        unreachable!()
    };
    assert_eq!(writer["genbank"], 1);
    let publication = basecraft_ops::provenance::inspect(&root.path().join("sample.gb")).unwrap();
    assert_eq!(&publication.writer, writer);
    for (version, damage_state, expected_problem) in [
        (Some(1), false, true),
        (Some(0), false, false),
        (Some(0), true, true),
        (None, false, true),
    ] {
        let replay_root = tempfile::tempdir().unwrap();
        let mut log = log::LogWriter::new(replay_root.path(), "fixture");
        for mut entry in entries.clone() {
            if let Op::Save { writer, .. } = &mut entry.op {
                if let Some(version) = version {
                    for v in writer.values_mut() {
                        *v = version;
                    }
                } else {
                    writer.clear();
                }
                entry.result.hashes[0].1 = "b3:wrong".into();
                if damage_state {
                    entry.state.len += 1;
                }
            }
            log.append(&entry).unwrap();
        }
        drop(log);
        let report = replay::replay(replay_root.path()).unwrap();
        assert_eq!(!report.ok(), expected_problem, "{report:?}");
        assert_eq!(
            report.writer_mismatches.len(),
            usize::from(version == Some(0))
        );
        if let Some(mismatch) = report.writer_mismatches.first() {
            assert_eq!(mismatch.code(), "writer_version_mismatch");
            assert!(mismatch.message().starts_with("written by an older writer"));
        }
    }
}

#[test]
fn companion_record_types_round_trip_original_writer_bytes() {
    fn check<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str) {
        let original = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/writers")
                .join(name),
        )
        .unwrap();
        let record: T = serde_json::from_slice(&original).unwrap();
        let mut encoded = serde_json::to_vec_pretty(&record).unwrap();
        if original.ends_with(b"\n") {
            encoded.push(b'\n');
        }
        assert_eq!(encoded, original);
    }
    check::<basecraft_protocol::companion_records::TreeSidecar>("tree-sidecar-tree-v1.bin");
    check::<basecraft_protocol::companion_records::DelimitationSidecar>(
        "delimitation-sidecar-legacy-v1.bin",
    );
}
