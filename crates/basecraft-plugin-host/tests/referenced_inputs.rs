//! Filesystem and public Python SDK boundary acceptance, without numerical mocks
//! masquerading as scientific validation. Real scientific tools have separate tests.
use basecraft_ops::{
    Session,
    analysis_store::ArtifactRun,
    input_evidence::{self, SavePolicy},
};
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
use basecraft_protocol::Op;
use std::path::Path;

fn fixture(root: &Path) -> std::path::PathBuf {
    let plugin = root.join("plugin");
    std::fs::create_dir(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.toml"), "schema = 1\nid = 'basecraft.mafft'\nname = 'Storage test'\nversion = 'test'\n[entry]\noperation = 'python:test.py'\n").unwrap();
    std::fs::write(
        plugin.join("test.py"),
        r#"
import json
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('basecraft.mafft', 'test')
@plugin.operation('align')
def align(snapshot, params):
    allocation = params['analysis_artifacts']
    root = Path(allocation['metadata_directory'])
    assert root.is_absolute() and root.is_dir()
    assert Path(allocation['working_directory']).is_absolute()
    assert allocation['run_id'] == params['job_id']
    source, target = Path(params['input']), Path(params['output'])
    target.write_bytes(source.read_bytes())
    receipt = root / 'sdk-allocation.json'
    receipt.write_text(json.dumps(allocation))
    return {'ops': [], 'outputs': [str(target), str(receipt)]}
plugin.run()
"#,
    )
    .unwrap();
    plugin
}

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
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("working");
    std::fs::create_dir(&folder).unwrap();
    let source = folder.join("input.fasta");
    // 16 MiB realistic FASTA, normalized the same way the submitted stream is.
    let sequence = "ACGT".repeat(125_000);
    let mut text = String::new();
    for name in 0..32 {
        text.push_str(&format!(">{name}\n"));
        for line in sequence.as_bytes().chunks(60) {
            text.push_str(std::str::from_utf8(line).unwrap());
            text.push('\n');
        }
    }
    std::fs::write(&source, &text).unwrap();
    let plugin = fixture(dir.path());
    let result = folder.join("aligned.fasta");
    let mut session = Session::recoverable(&folder).unwrap();
    let doc = session.open(&source).unwrap();
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "align"),
        AlignmentInput::Ungapped,
        result.clone(),
    )
    .unwrap();
    let outcome = job.run();
    job.record_terminal(&mut session, doc, &outcome, 0.0)
        .unwrap();
    job.record(&mut session, doc, &outcome.unwrap()).unwrap();
    let io = job.input_io().unwrap();
    assert_eq!(io["source_identity_hash_bytes"], 0);
    assert_eq!(io["persistent_original_copy_bytes"], 0);
    assert_eq!(io["persistent_submitted_copy_bytes"], 0);
    assert_eq!(io["execution_scratch_read_bytes"], text.len() as u64);
    let run = ArtifactRun::for_result(&result).unwrap().unwrap();
    assert!(!run.join("submitted.fasta").exists());
    assert!(!run.join("preserved-input").exists());
    assert!(!folder.join(".basecraft/project.toml").exists());
    let mut bytes = 0;
    for path in files(&folder.join(".basecraft")) {
        if path.starts_with(run.join("pending")) {
            continue;
        } // Output hardlink, no input copy.
        bytes += std::fs::metadata(path).unwrap().len();
    }
    assert!(
        bytes < 100_000,
        "metadata footprint {bytes} bytes for {} input bytes",
        text.len()
    );
    assert!(
        basecraft_ops::provenance::verify(&result)
            .unwrap()
            .is_empty()
    );
    println!(
        "input_bytes={} metadata_bytes={bytes} host_input_io={io}",
        text.len()
    );
    drop(job);
    drop(session);
    let moved = dir.path().join("moved");
    std::fs::rename(&folder, &moved).unwrap();
    let result = moved.join("aligned.fasta");
    assert!(
        basecraft_ops::provenance::verify(&result)
            .unwrap()
            .is_empty()
    );
    let package = dir.path().join("package");
    basecraft_ops::provenance::package(&result, &package).unwrap();
    std::fs::remove_file(moved.join("input.fasta")).unwrap();
    assert!(
        !basecraft_ops::provenance::verify(&result)
            .unwrap()
            .is_empty()
    );
    assert!(
        basecraft_ops::provenance::verify(&package.join("aligned.fasta"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn common_save_route_preserves_actual_original_and_dirty_submitted_input() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let old = std::fs::read(&source).unwrap();
    let plugin = fixture(dir.path());
    let output = dir.path().join("out.fasta");
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "align"),
        AlignmentInput::Ungapped,
        output.clone(),
    )
    .unwrap();
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
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
    let dirty = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "align"),
        AlignmentInput::Ungapped,
        dirty_output.clone(),
    )
    .unwrap();
    let result = dirty.run().unwrap();
    dirty.record(&mut session, doc, &result).unwrap();
    let run = ArtifactRun::for_result(&dirty_output).unwrap().unwrap();
    assert_eq!(input_evidence::read_input(&run).unwrap().state, "generated");
    assert!(
        std::fs::read_to_string(run.join("submitted.fasta"))
            .unwrap()
            .contains("TAAA")
    );
    assert!(
        dirty.input_io().unwrap()["persistent_submitted_copy_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    // Save As leaves old evidence intact and never needs a decision for a new path.
    session
        .save(doc, Some(&dir.path().join("new.fasta")))
        .unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), old);
    session
        .save_with_evidence(doc, None, SavePolicy::PreserveCopy)
        .unwrap();
    let run = ArtifactRun::for_result(&output).unwrap().unwrap();
    assert_eq!(std::fs::read(run.join("preserved-input")).unwrap(), old);
    assert!(
        basecraft_ops::provenance::verify(&output)
            .unwrap()
            .is_empty()
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
    assert!(
        basecraft_ops::provenance::verify(&copied_result)
            .unwrap()
            .is_empty()
    );
    assert!(basecraft_ops::replay::replay(dir.path()).unwrap().ok());
}

#[test]
fn downstream_evidence_follows_preservation_and_complete_export_keeps_dependency_closure() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let plugin = fixture(root.path());
    let mut session = Session::recoverable(root.path()).unwrap();
    let first = root.path().join("first.afa");
    let second = root.path().join("second.afa");
    for (source, output) in [(&source, &first), (&first, &second)] {
        let doc = session.open(source).unwrap();
        let job = FileJob::from_alignment(
            &session,
            doc,
            (&plugin, "align"),
            AlignmentInput::Ungapped,
            output.clone(),
        )
        .unwrap();
        let result = job.run().unwrap();
        job.record(&mut session, doc, &result).unwrap();
    }
    let conflict = input_evidence::conflicts(&source).unwrap();
    assert_eq!(conflict.analyses.len(), 2);
    assert_eq!(conflict.analyses.iter().filter(|a| a.direct).count(), 1);
    input_evidence::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::remove_file(&source).unwrap();
    assert!(
        basecraft_ops::provenance::verify(&second)
            .unwrap()
            .is_empty()
    );
    let package = root.path().join("portable");
    basecraft_ops::provenance::package(&second, &package).unwrap();
    let first_run = ArtifactRun::for_result(&first).unwrap().unwrap();
    std::fs::write(first_run.join("preserved-input"), ">a\nTTTT\n>b\nTTTT\n").unwrap();
    assert!(
        !basecraft_ops::provenance::verify(&second)
            .unwrap()
            .is_empty()
    );
    std::fs::remove_file(first).unwrap();
    assert!(
        basecraft_ops::provenance::verify(&package.join("second.afa"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn save_as_descendants_receive_each_explicit_evidence_decision() {
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
        let plugin = fixture(root.path());
        let mut session = Session::recoverable(root.path()).unwrap();
        for (input, output) in [(&source, &first), (&saved, &last)] {
            let doc = session.open(input).unwrap();
            let job = FileJob::from_alignment(
                &session,
                doc,
                (&plugin, "align"),
                AlignmentInput::Ungapped,
                output.clone(),
            )
            .unwrap();
            let outcome = job.run().unwrap();
            job.record(&mut session, doc, &outcome).unwrap();
            if output == &first {
                let doc = session.open(&first).unwrap();
                session.save(doc, Some(&saved)).unwrap();
            }
        }
        let conflict = input_evidence::conflicts(&source).unwrap();
        assert_eq!(conflict.analyses.len(), 2, "{conflict:?}");
        input_evidence::resolve(&conflict, policy).unwrap();
        std::fs::write(&source, ">a\nTTTT\n>b\nTTTA\n").unwrap();
        match policy {
            SavePolicy::PreserveCopy => {
                assert!(basecraft_ops::provenance::verify(&last).unwrap().is_empty())
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
                        .all(|a| input_evidence::read_input(&a.run).unwrap().state == "broken")
                );
                assert!(!basecraft_ops::provenance::verify(&last).unwrap().is_empty());
            }
            SavePolicy::Cancel => unreachable!(),
        }
    }
}

#[test]
fn complete_export_remaps_history_and_retains_original_records_without_store() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let first = root.path().join("first.afa");
    let saved = root.path().join("saved.afa");
    let plugin = fixture(root.path());
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "align"),
        AlignmentInput::Ungapped,
        first.clone(),
    )
    .unwrap();
    job.record(&mut session, doc, &job.run().unwrap()).unwrap();
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
    let bundle = basecraft_ops::provenance::bundle_for_result(&saved);
    let originals: Vec<_> = std::fs::read_dir(bundle.join("records"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect();
    let package = root.path().join("portable");
    basecraft_ops::provenance::package(&saved, &package).unwrap();
    let exported = package.join("saved.afa");
    let exported_bundle = basecraft_ops::provenance::bundle_for_result(&exported);
    let publication = basecraft_ops::provenance::inspect(&exported).unwrap();
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
    assert!(
        basecraft_ops::provenance::verify(&exported)
            .unwrap()
            .is_empty()
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
    assert!(
        !basecraft_ops::provenance::verify(&exported)
            .unwrap()
            .is_empty()
    );
}
