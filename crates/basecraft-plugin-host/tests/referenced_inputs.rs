//! Filesystem and public Python SDK boundary acceptance for registered runs,
//! without numerical mocks masquerading as scientific validation. Real
//! scientific tools have separate tests.
mod common;
#[path = "../../basecraft-ops/tests/support/decided_save.rs"]
mod decided_save;
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{
    Session,
    records::ArtifactRun,
    records::{self, SavePolicy},
};
use basecraft_protocol::Op;
use std::{fmt::Write as _, path::Path};

fn files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            out.extend(files(&entry.path()));
        } else {
            out.push(entry.path());
        }
    }
    out
}

#[test]
fn synthetic_large_clean_source_has_metadata_only_evidence_and_relocates() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("working");
    std::fs::create_dir(&folder).unwrap();
    let source = folder.join("input.fasta");
    // 16 MiB realistic FASTA, normalized the same way the submitted stream is.
    let sequence = "ACGT".repeat(125_000);
    let mut text = String::new();
    for name in 0..32 {
        writeln!(text, ">{name}").unwrap();
        for line in sequence.as_bytes().chunks(60) {
            text.push_str(std::str::from_utf8(line).unwrap());
            text.push('\n');
        }
    }
    std::fs::write(&source, &text).unwrap();
    let registry = common::copy_aligner(dir.path());
    let result = folder.join("aligned.fasta");
    let mut session = Session::recoverable(&folder).unwrap();
    common::align_copy(&registry, &mut session, &source, &result);
    let run = ArtifactRun::for_result(&result).unwrap().unwrap();
    // The clean source is referenced in place: its bytes are never copied.
    assert_eq!(
        records::read_input(&run).unwrap().state,
        basecraft_protocol::input_records::EvidenceState::Referenced
    );
    assert!(!run.join("submitted.fasta").exists());
    assert!(!run.join("preserved-input").exists());
    assert!(!folder.join(".basecraft/project.toml").exists());
    let mut bytes = 0;
    for path in files(&folder.join(".basecraft")) {
        if path.starts_with(run.join("artifacts")) {
            continue;
        } // The retained result itself, not input evidence.
        bytes += std::fs::metadata(path).unwrap().len();
    }
    assert!(
        bytes < 100_000,
        "metadata footprint {bytes} bytes for {} input bytes",
        text.len()
    );
    assert_eq!(
        basecraft_ops::records::verify(&result).unwrap(),
        Vec::<String>::new()
    );
    println!("input_bytes={} metadata_bytes={bytes}", text.len());
    drop(session);
    let moved = dir.path().join("moved");
    std::fs::rename(&folder, &moved).unwrap();
    let result = moved.join("aligned.fasta");
    assert_eq!(
        basecraft_ops::records::verify(&result).unwrap(),
        Vec::<String>::new()
    );
    let package = dir.path().join("package");
    basecraft_ops::records::package(&result, &package).unwrap();
    std::fs::remove_file(moved.join("input.fasta")).unwrap();
    assert_ne!(
        basecraft_ops::records::verify(&result).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        basecraft_ops::records::verify(&package.join("aligned.fasta")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn common_save_route_preserves_actual_original_and_dirty_submitted_input() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let old = std::fs::read(&source).unwrap();
    let registry = common::copy_aligner(dir.path());
    let output = dir.path().join("out.fasta");
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    common::align_document(&registry, &mut session, doc, &output);
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    let conflict = session.save(doc, None).unwrap_err();
    assert!(matches!(
        conflict,
        basecraft_ops::Error::EvidenceConflict(_)
    ));
    assert_eq!(std::fs::read(&source).unwrap(), old);
    let dirty_output = dir.path().join("dirty.fasta");
    let dirty = common::align_document(&registry, &mut session, doc, &dirty_output);
    let run = ArtifactRun::for_result(&dirty_output).unwrap().unwrap();
    assert_eq!(run, common::evidence(&dirty));
    assert_eq!(
        records::read_input(&run).unwrap().state,
        basecraft_protocol::input_records::EvidenceState::Generated
    );
    // The unsaved bytes that were analysed are retained with the run.
    assert!(
        std::fs::read_to_string(run.join("submitted.fasta"))
            .unwrap()
            .contains("TAAA")
    );
    // Save As leaves old evidence intact and never needs a decision for a new path.
    session
        .save(doc, Some(&dir.path().join("new.fasta")))
        .unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), old);
    decided_save::save_with_policy(&mut session, doc, None, SavePolicy::PreserveCopy).unwrap();
    let run = ArtifactRun::for_result(&output).unwrap().unwrap();
    assert_eq!(std::fs::read(common::verified_input(&run)).unwrap(), old);
    assert_eq!(
        basecraft_ops::records::verify(&output).unwrap(),
        Vec::<String>::new()
    );
    let result_doc = session.open(&output).unwrap();
    session
        .apply(
            result_doc,
            Op::AlnReplace {
                row: 1,
                start: 0,
                end: 1,
                residues: "C".into(),
            },
        )
        .unwrap();
    let copied_result = dir.path().join("copied-result.fasta");
    session.save(result_doc, Some(&copied_result)).unwrap();
    assert_eq!(
        basecraft_ops::records::verify(&copied_result).unwrap(),
        Vec::<String>::new()
    );
    let report = basecraft_ops::records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn downstream_evidence_follows_preservation_and_complete_export_keeps_dependency_closure() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let registry = common::copy_aligner(root.path());
    let mut session = Session::recoverable(root.path()).unwrap();
    let first = root.path().join("first.afa");
    let second = root.path().join("second.afa");
    for (source, output) in [(&source, &first), (&first, &second)] {
        common::align_copy(&registry, &mut session, source, output);
    }
    let conflict = records::conflicts(&source).unwrap();
    let analyses = common::analyses(&conflict);
    assert_eq!(analyses.len(), 2);
    assert_eq!(analyses.iter().filter(|a| a.direct).count(), 1);
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::remove_file(&source).unwrap();
    assert_eq!(
        basecraft_ops::records::verify(&second).unwrap(),
        Vec::<String>::new()
    );
    let package = root.path().join("portable");
    basecraft_ops::records::package(&second, &package).unwrap();
    let first_run = ArtifactRun::for_result(&first).unwrap().unwrap();
    std::fs::write(common::verified_input(&first_run), ">a\nTTTT\n>b\nTTTT\n").unwrap();
    assert_ne!(
        basecraft_ops::records::verify(&second).unwrap(),
        Vec::<String>::new()
    );
    std::fs::remove_file(first).unwrap();
    assert_eq!(
        basecraft_ops::records::verify(&package.join("second.afa")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn save_as_descendants_receive_each_explicit_evidence_decision() {
    crate::isolated_profile::isolate!();
    for policy in [
        SavePolicy::PreserveCopy,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.afa");
        std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
        let first = root.path().join("first.afa");
        let saved = root.path().join("saved.afa");
        let last = root.path().join("last.afa");
        let registry = common::copy_aligner(root.path());
        let mut session = Session::recoverable(root.path()).unwrap();
        for (input, output) in [(&source, &first), (&saved, &last)] {
            common::align_copy(&registry, &mut session, input, output);
            if output == &first {
                let doc = session.open(&first).unwrap();
                session.save(doc, Some(&saved)).unwrap();
            }
        }
        let conflict = records::conflicts(&source).unwrap();
        assert_eq!(common::analyses(&conflict).len(), 2, "{conflict:?}");
        records::resolve(&conflict, policy).unwrap();
        std::fs::write(&source, ">a\nTTTT\n>b\nTTTA\n").unwrap();
        match policy {
            SavePolicy::PreserveCopy => {
                assert_eq!(
                    basecraft_ops::records::verify(&last).unwrap(),
                    Vec::<String>::new()
                );
            }
            SavePolicy::RemoveAnalysis => {
                assert!(conflict.analyses.iter().all(|a| !a.run.exists()));
                assert!(first.is_file() && saved.is_file() && last.is_file());
            }
            SavePolicy::BreakEvidence => {
                assert!(
                    conflict
                        .analyses
                        .iter()
                        .all(|a| records::read_input(&a.run).unwrap().state
                            == basecraft_protocol::input_records::EvidenceState::Broken)
                );
                assert_ne!(
                    basecraft_ops::records::verify(&last).unwrap(),
                    Vec::<String>::new()
                );
            }
            SavePolicy::Cancel => unreachable!(),
        }
    }
}

#[test]
fn complete_export_remaps_history_and_retains_original_records_without_store() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let first = root.path().join("first.afa");
    let saved = root.path().join("saved.afa");
    let registry = common::copy_aligner(root.path());
    let mut session = Session::recoverable(root.path()).unwrap();
    common::align_copy(&registry, &mut session, &source, &first);
    let doc = session.open(&first).unwrap();
    for residue in ["T", "C"] {
        session
            .apply(
                doc,
                Op::AlnReplace {
                    row: 0,
                    start: 0,
                    end: 1,
                    residues: residue.into(),
                },
            )
            .unwrap();
        session.save(doc, None).unwrap();
    }
    session.save(doc, Some(&saved)).unwrap();
    let bundle = basecraft_ops::records::bundle_for_result(&saved);
    let originals: Vec<_> = std::fs::read_dir(bundle.join("records"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect();
    let package = root.path().join("portable");
    basecraft_ops::records::package(&saved, &package).unwrap();
    let exported = package.join("saved.afa");
    let exported_bundle = basecraft_ops::records::bundle_for_result(&exported);
    let publication = basecraft_ops::records::inspect(&exported).unwrap();
    for bytes in originals {
        let hash = basecraft_io::hash_bytes(&bytes);
        let artifact = publication
            .inputs
            .iter()
            .find(|a| a.hash == hash)
            .expect("original immutable JSON retained");
        assert_eq!(
            std::fs::read(exported_bundle.join(artifact.retained.as_ref().unwrap())).unwrap(),
            bytes
        );
    }
    std::fs::remove_file(source).unwrap();
    std::fs::rename(
        root.path().join(".basecraft"),
        root.path().join("unavailable-store"),
    )
    .unwrap();
    std::fs::rename(bundle, root.path().join("unavailable-companion")).unwrap();
    assert_eq!(
        basecraft_ops::records::verify(&exported).unwrap(),
        Vec::<String>::new()
    );
    // Archived immutable records participate in package verification too.
    let archive = publication
        .inputs
        .iter()
        .find(|a| a.logical_name.starts_with("original-record-"))
        .unwrap();
    std::fs::write(
        exported_bundle.join(archive.retained.as_ref().unwrap()),
        "corrupt",
    )
    .unwrap();
    assert_ne!(
        basecraft_ops::records::verify(&exported).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn one_save_decision_covers_every_backing_reference_without_merging_analyses() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    let old = b">a\nAAAA\n>b\nAAAT\n";
    std::fs::write(&source, old).unwrap();
    let registry = common::copy_aligner(root.path());
    let mut session = Session::recoverable(root.path()).unwrap();
    let first = root.path().join("first.afa");
    let second = root.path().join("second.afa");
    common::align_copy(&registry, &mut session, &source, &first);
    common::align_copy(&registry, &mut session, &source, &second);
    let conflict = records::conflicts(&source).unwrap();
    assert_eq!(
        conflict.analyses.len(),
        2,
        "distinct analyses stay distinct"
    );
    for analysis in &conflict.analyses {
        assert_eq!(analysis.backing_analyses.len(), 2, "run plus input ledger");
    }
    assert!(records::resolve(&conflict, SavePolicy::Cancel).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), old);
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::remove_file(&source).unwrap();
    // Retries verify retained bytes, even after the source has disappeared.
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    for analysis in &conflict.analyses {
        assert_eq!(
            analysis
                .backing_analyses
                .iter()
                .filter(|a| a.run.join("preserved-input").exists())
                .count(),
            1
        );
        for backing in &analysis.backing_analyses {
            let input = records::read_input(&backing.run).unwrap();
            let retained =
                records::verified_source(&backing.run, input.source.as_ref().unwrap()).unwrap();
            assert_eq!(std::fs::read(retained).unwrap(), old);
        }
    }
    for path in [&first, &second] {
        assert_eq!(
            basecraft_ops::records::verify(path).unwrap(),
            Vec::<String>::new()
        );
    }
    let original = common::verified_input(&conflict.analyses[0].run);
    std::fs::write(original, "damaged").unwrap();
    assert!(records::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
}
