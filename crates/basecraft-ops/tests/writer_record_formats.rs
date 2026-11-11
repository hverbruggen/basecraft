use basecraft_ops::{files, metadata, submission};
use basecraft_protocol::metadata::{
    Cell, Column, ColumnKind, Identification, ProjectMetadata, RecordLink, Row, Table, Taxon,
};
use basecraft_protocol::submission::{
    AccessionEntry, Author, Decision, FrameChoice, Publication, RowDecision, SourceAlignment,
    Submission,
};
use std::path::Path;

fn golden(format: &str, variant: &str, path: &Path, bytes: Vec<u8>) {
    let version = writer_versions(path)[format];
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
        use basecraft_ops::records::{AlignmentLink, AlignmentLinkExt};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.afa");
        let tree = root.path().join("tree.nwk");
        std::fs::write(&source, ">a\nAC\n>b\nAT\n").unwrap();
        std::fs::write(&tree, "(a,b);\n").unwrap();
        AlignmentLink::create(&tree, &source, &source, 7, true, None).unwrap();
        let bytes = std::fs::read_to_string(basecraft_ops::records::sidecar(&tree)).unwrap();
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
    let metadata_fixture = ProjectMetadata {
        specimens: Table {
            columns: vec![
                Column {
                    name: "locality".into(),
                    kind: ColumnKind::Text,
                    description: "Collection site".into(),
                },
                Column {
                    name: "depth".into(),
                    kind: ColumnKind::Number {
                        unit: Some("m".into()),
                    },
                    description: String::new(),
                },
            ],
            rows: vec![
                Row {
                    id: "S1".into(),
                    cells: [
                        ("locality".into(), Cell::Text("Île\tNorth\nshore".into())),
                        ("depth".into(), Cell::Number(2.5)),
                    ]
                    .into(),
                },
                Row {
                    id: "S2".into(),
                    cells: std::collections::BTreeMap::new(),
                },
            ],
        },
        taxa: vec![Taxon {
            id: "T1".into(),
            name: "Caulerpa taxifolia".into(),
            submitted_name: None,
            taxid: Some(76317),
        }],
        identifications: vec![Identification {
            id: "I1".into(),
            specimen: "S1".into(),
            taxon: "T1".into(),
            identified_by: Some("Example, A.".into()),
            date: Some(basecraft_protocol::metadata::PartialDate::parse("2026-01").unwrap()),
            remarks: Some("reviewed".into()),
        }],
        links: vec![RecordLink {
            document: "locus.afa".into(),
            record: "sample".into(),
            specimen: "S1".into(),
        }],
        ..Default::default()
    };
    for (i, bytes) in metadata::encode(&metadata_fixture)
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
            &format!("filled-{i}"),
            Path::new("metadata.basecraft.json"),
            bytes,
        );
    }
    let submission_fixture = Submission {
        source: Some(SourceAlignment {
            document: "locus.afa".into(),
            hash: "b3:source".into(),
        }),
        rows: vec![RowDecision {
            record: "sample".into(),
            decision: Decision::New,
        }],
        frames: vec![FrameChoice {
            record: "sample".into(),
            codon_start: 2,
        }],
        publication: Publication {
            title: "Synthetic marker study".into(),
            authors: vec![Author {
                first: "Ana".into(),
                last: "Example".into(),
                middle: "M.".into(),
            }],
        },
        accessions: vec![AccessionEntry {
            record: "sample".into(),
            accession: "AB123456.1".into(),
        }],
        ..Default::default()
    };
    golden(
        "submission",
        "filled",
        Path::new("sample.submission.json"),
        submission::encode(&submission_fixture).unwrap(),
    );
}

#[test]
fn replay_checks_saved_bytes_and_document_fingerprints() {
    use basecraft_ops::{Session, log, records};
    use basecraft_protocol::Op;
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path());
    let doc = session.create_sequence("sample", "ACGT").unwrap();
    session
        .save(doc, Some(&root.path().join("sample.gb")))
        .unwrap();
    drop(session);
    let entries = log::read_all(root.path()).unwrap();
    for damage_state in [false, true] {
        let replay_root = tempfile::tempdir().unwrap();
        let mut log = log::LogWriter::new(replay_root.path(), "fixture");
        for mut entry in entries.clone() {
            if let Op::Save { .. } = &mut entry.op {
                entry.result.hashes[0].1 = "b3:wrong".into();
                if damage_state {
                    entry.state.len += 1;
                }
            }
            log.append(&entry).unwrap();
        }
        drop(log);
        let report = records::replay(replay_root.path()).unwrap();
        assert!(!report.ok(), "{report:?}");
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("saved content differs"))
        );
        assert_eq!(
            report.problems.iter().any(|p| p.contains("state ")),
            damage_state
        );
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
        "delimitation-sidecar-current-v1.bin",
    );
}

// Only current writer goldens are kept; the journal has no old-writer exceptions.
fn writer_versions(path: &Path) -> std::collections::BTreeMap<String, u32> {
    use basecraft_ops::files::Format;
    let formats: &[(&str, u32)] = match Format::from_path(path) {
        Ok(Format::Fasta) => &[("fasta", 1), ("gff3", 1)],
        Ok(Format::GenBank) => &[("genbank", 1)],
        Ok(Format::Tree) => &[("newick", 1), ("tree-sidecar", 1), ("alignment-link", 1)],
        Ok(Format::Delimitation) => &[("spart", 1), ("delimitation-sidecar", 1)],
        Ok(Format::Metadata) => &[("metadata-manifest", 1), ("metadata-tsv", 1)],
        Ok(Format::Submission) => &[("submission", 1)],
        Ok(Format::SangerReview) => &[("sanger-review", 3)],
        Err(_) => &[],
    };
    formats
        .iter()
        .map(|(name, version)| ((*name).to_owned(), *version))
        .collect()
}
