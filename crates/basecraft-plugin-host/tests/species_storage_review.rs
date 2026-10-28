//! Independent storage review probes and regression coverage for their corrections.
// Preserve the independently reviewed assertions verbatim.
#![allow(clippy::assert_is_empty)]
mod common;

use basecraft_ops::{Session, files, provenance};
use basecraft_plugin_host::registry::Registry;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Create the species workspace `output` from the open alignment `input`.
fn setup_alignment(
    registry: &Registry,
    session: &mut Session,
    input: basecraft_ops::DocId,
    output: &Path,
) {
    let outcome = common::run_in(registry, session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_alignment@1",
            "inputs":{"alignment":common::document(s, input)},
            "parameters":{},"destinations":{"workspace":output}})
    });
    common::assert_completed(&outcome);
}

/// The source of a run's referenced input, as its input evidence records it.
fn referenced_hash(outcome: &basecraft_protocol::operation::OperationOutcome) -> String {
    basecraft_ops::input_evidence::read_input(&common::evidence(outcome))
        .unwrap()
        .source
        .unwrap()
        .hash
}

#[test]
fn rereview_alignment_move_and_explicit_evidence_decisions() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    for policy in [SavePolicy::BreakEvidence, SavePolicy::RemoveAnalysis] {
        let parent = tempfile::tempdir().unwrap();
        let folder = parent.path().join("working");
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("source.afa");
        let original = ">a\nAA\nAA\n>b\nAA\nAC\n>c\nAA\nCC\n>d\nCC\nCC\n";
        std::fs::write(&source, original).unwrap();
        let canonical = folder.join("species.spart");
        let registry = common::bundled();
        let mut session = Session::recoverable(&folder).unwrap();
        let input = session.open(&source).unwrap();
        setup_alignment(&registry, &mut session, input, &canonical);
        drop(session);
        let moved = parent.path().join("moved");
        std::fs::rename(&folder, &moved).unwrap();
        let source = moved.join("source.afa");
        let canonical = moved.join("species.spart");
        let work = files::load_delimitation(&canonical).unwrap();
        let checked = || basecraft_ops::delimitation::checked_alignment(&work, &canonical);
        assert_eq!(checked().unwrap().unwrap().canonicalize().unwrap(), source);
        std::fs::write(&source, ">a\nTTTT\n>b\nTTTC\n>c\nTTCC\n>d\nCCCC\n").unwrap();
        assert!(checked().is_err(), "changed unpreserved input must fail");
        std::fs::remove_file(&source).unwrap();
        assert!(checked().is_err(), "missing unpreserved input must fail");
        std::fs::write(&source, original).unwrap();
        assert!(
            checked().is_ok(),
            "accepted bytes remain usable after restoration"
        );
        let conflict = conflicts(&source).unwrap();
        assert!(conflict.analyses.iter().any(|a| a.direct));
        resolve(&conflict, policy).unwrap();
        assert!(checked().is_err(), "explicit {policy:?} must prevent reuse");
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        assert!(canonical.is_file());
        assert!(provenance::package(&canonical, &parent.path().join("rejected")).is_err());
    }
}

#[test]
fn rereview_legacy_sidecar_break_and_remove_reach_combined_workspace() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, read_input, resolve};
    for policy in [SavePolicy::BreakEvidence, SavePolicy::RemoveAnalysis] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("legacy.spart");
        let original = include_str!("../../../testdata/fixtures/delimitation/manual.spart");
        std::fs::write(&source, original).unwrap();
        let work = files::load_delimitation(&source).unwrap();
        let mut bytes = Vec::new();
        files::write_delimitation_sidecar(
            &work,
            &basecraft_io::hash_file(&source).unwrap(),
            &mut bytes,
        )
        .unwrap();
        let sidecar = files::tree_sidecar(&source);
        std::fs::write(&sidecar, &bytes).unwrap();
        let target = root.path().join("combined.spart");
        std::fs::copy(&source, &target).unwrap();
        let mut session = Session::recoverable(root.path()).unwrap();
        let doc = session.open(&target).unwrap();
        basecraft_ops::delimitation::combine(&mut session, doc, std::slice::from_ref(&source))
            .unwrap();
        session.save(doc, None).unwrap();
        assert!(provenance::verify(&target).unwrap().is_empty());
        let conflict = conflicts(&sidecar).unwrap();
        assert!(conflict.analyses.iter().any(|a| a.direct));
        assert!(conflict.analyses.iter().any(|a| !a.direct));
        resolve(&conflict, policy).unwrap();
        for affected in &conflict.analyses {
            match policy {
                SavePolicy::BreakEvidence => {
                    assert_eq!(read_input(&affected.run).unwrap().state, "broken");
                }
                SavePolicy::RemoveAnalysis => assert!(!affected.run.exists()),
                _ => unreachable!(),
            }
        }
        assert!(
            !matches!(provenance::verify(&target), Ok(issues) if issues.is_empty()),
            "explicit {policy:?} must never verify complete"
        );
        assert!(provenance::package(&target, &root.path().join("rejected")).is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), bytes);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        assert!(
            !files::load_delimitation(&target)
                .unwrap()
                .hypotheses
                .is_empty()
        );
    }
}

fn setup_tree(session: &mut Session, source: &Path, output: &Path) {
    let doc = session.open(source).unwrap();
    let outcome = common::run_in(&common::bundled(), session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::document(s, doc)},
            "parameters":{},"destinations":{"workspace":output}})
    });
    common::assert_completed(&outcome);
}

/// Fit single-threshold GMYC to `tree`, a project file, in the workspace `doc`.
fn fit_in_workspace(session: &mut Session, doc: basecraft_ops::DocId, tree: &Path) {
    let relative = tree.strip_prefix(session.root()).unwrap().to_owned();
    let outcome = common::run_in(&common::bundled(), session, |s| {
        json!({"operation":"basecraft.delimitation/gmyc_single@1",
            "inputs":{"tree":common::file(relative.to_str().unwrap())},
            "parameters":{"rooted":true},"target":common::document(s, doc)[0]})
    });
    common::assert_completed(&outcome);
}

#[test]
fn nexus_source_workspace_can_reuse_its_display_tree() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nex");
    std::fs::write(
        &source,
        "#NEXUS\nBEGIN TREES;\nTREE t = ((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\nEND;\n",
    )
    .unwrap();
    let canonical = root.path().join("species.spart");
    let mut session = Session::recoverable(root.path()).unwrap();
    setup_tree(&mut session, &source, &canonical);
    let doc = session.open(&canonical).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(session.delimitation(doc))
        .unwrap()
        .unwrap();
    // The controller parses this checked display buffer as Newick on reopen.
    let bytes = std::fs::read_to_string(&display).unwrap();
    let parsed = basecraft_io::newick::parse_trees(&bytes);
    assert!(
        parsed.is_ok(),
        "checked display must be usable Newick: {display:?}: {parsed:?}"
    );
    // The workspace must reuse its accepted normalized tree.
    fit_in_workspace(&mut session, doc, &display);
}

#[test]
fn alignment_gmyc_export_preserves_display_and_declared_inputs() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.afa");
    std::fs::write(
        &source,
        format!(
            ">a\n{}\n>b\n{}\n>c\n{}{}\n>d\n{}{}\n>e\n{}{}\n",
            "A".repeat(100),
            "A".repeat(100),
            "A".repeat(90),
            "C".repeat(10),
            "A".repeat(80),
            "G".repeat(20),
            "A".repeat(70),
            "T".repeat(30)
        ),
    )
    .unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&source).unwrap();
    let outcome = common::run_in(&common::bundled(), &mut session, |s| {
        json!({"operation":"basecraft.host/gmyc_alignment_single@1",
            "inputs":{"alignment":common::document(s, doc)},
            "parameters":{"unique_haplotypes":true,"model":"p"},
            "destinations":{"workspace":"species.spart"}})
    });
    common::assert_completed(&outcome);
    let canonical = root.join("species.spart");
    let package = parent.path().join("package");
    provenance::package(&canonical, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let moved = parent.path().join("moved-package");
    std::fs::rename(&package, &moved).unwrap();
    let exported = moved.join("species.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let work = files::load_delimitation(&exported).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(&work)
        .unwrap()
        .unwrap();
    assert!(
        display.is_file(),
        "exported display is unavailable: {display:?}"
    );
    for hypothesis in &work.hypotheses {
        for (path, hash) in &hypothesis.provenance.inputs {
            assert_eq!(
                &basecraft_io::hash_file(Path::new(path)).unwrap(),
                hash,
                "exported input {path}"
            );
        }
    }
}

#[test]
fn preserved_tree_export_relocates_original_display_reference() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let canonical = root.join("species.spart");
    let mut session = Session::recoverable(&root).unwrap();
    setup_tree(&mut session, &source, &canonical);
    resolve(&conflicts(&source).unwrap(), SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, "(changed:1,input:1);\n").unwrap();
    let original = files::load_delimitation(&canonical).unwrap();
    assert!(
        basecraft_ops::delimitation::checked_display_tree(&original)
            .unwrap()
            .unwrap()
            .is_file()
    );
    let package = parent.path().join("package");
    provenance::package(&canonical, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let exported = package.join("species.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let work = files::load_delimitation(&exported).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(&work);
    assert!(
        display.is_ok(),
        "complete export lost preserved display: {display:?}"
    );
    assert!(display.unwrap().unwrap().is_file());
}

#[test]
fn fitted_workspace_survives_folder_move_then_save_and_export() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let canonical = root.join("species.spart");
    let mut session = Session::recoverable(&root).unwrap();
    setup_tree(&mut session, &source, &canonical);
    let doc = session.open(&canonical).unwrap();
    fit_in_workspace(&mut session, doc, &source);
    session.save(doc, None).unwrap();
    drop(session);
    let moved = parent.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    let canonical = moved.join("species.spart");
    let mut session = Session::recoverable(&moved).unwrap();
    let doc = session.open(&canonical).unwrap();
    session.save(doc, None).unwrap();
    assert!(provenance::verify(&canonical).unwrap().is_empty());
    provenance::package(&canonical, &parent.path().join("package")).unwrap();
}

#[test]
fn preserved_alignment_remains_usable_by_workspace_default_input() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    // Raw source identity differs from the canonical FASTA submitted by setup.
    std::fs::write(&source, ">a\nAA\nAA\n>b\nAA\nAC\n>c\nAA\nCC\n>d\nCC\nCC\n").unwrap();
    let canonical = root.path().join("species.spart");
    let registry = common::bundled();
    let mut session = Session::recoverable(root.path()).unwrap();
    let input = session.open(&source).unwrap();
    setup_alignment(&registry, &mut session, input, &canonical);
    let clean = files::load_delimitation(&canonical).unwrap();
    let owner = PathBuf::from(&clean.exchange_fields["basecraft_alignment_evidence_run"]);
    // The clean source is referenced in place; no copy of it is retained.
    assert!(!owner.join("preserved-input").exists());
    assert!(!owner.join("submitted.fasta").exists());
    assert!(!owner.join("document-input.afa").exists());
    assert_eq!(
        basecraft_ops::input_evidence::read_input(&owner)
            .unwrap()
            .state,
        "referenced"
    );
    assert_eq!(
        basecraft_ops::delimitation::checked_alignment(&clean, &canonical)
            .unwrap()
            .unwrap()
            .canonicalize()
            .unwrap(),
        source.canonicalize().unwrap()
    );
    resolve(&conflicts(&source).unwrap(), SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTC\n>c\nTTCC\n>d\nCCCC\n").unwrap();
    assert!(provenance::verify(&canonical).unwrap().is_empty());
    let doc = session.open(&canonical).unwrap();
    let work = session.delimitation(doc).clone();
    // The controller resolves the linked default through the shared lifecycle,
    // including when it submits an all-specimen display-tree job.
    let linked = basecraft_ops::delimitation::checked_alignment(&work, &canonical)
        .unwrap()
        .unwrap();
    let expected = work.exchange_fields["basecraft_alignment_hash"].clone();
    let relative = linked.strip_prefix(root.path()).unwrap().to_owned();
    let relative = relative.to_str().unwrap();
    let preparation = common::run_in(&registry, &mut session, |_| {
        json!({"operation":"basecraft.delimitation/haplotypes@1",
            "inputs":{"alignment":common::file(relative)},"parameters":{}})
    });
    common::assert_completed(&preparation);
    assert_eq!(
        referenced_hash(&preparation),
        expected,
        "GUI default must follow the verified preserved alignment, as checked_display_tree does for trees"
    );
    let display = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.distance/tree@1",
            "inputs":{"alignment":common::file(relative)},
            "parameters":{"method":"upgma","model":"p"},
            "destinations":{"artifacts/tree.nwk":"display.nwk"},
            "target":common::document(s, doc)[0]})
    });
    common::assert_completed(&display);
    assert_eq!(
        referenced_hash(&display),
        expected,
        "display job must reuse the preserved alignment"
    );
    std::fs::remove_file(&source).unwrap();
    let mut old = work.clone();
    old.exchange_fields
        .remove("basecraft_alignment_evidence_run");
    assert_eq!(
        basecraft_ops::delimitation::checked_alignment(&old, &canonical)
            .unwrap()
            .unwrap(),
        linked
    );
    session.save(doc, None).unwrap();
    let copied = root.path().join("copied.spart");
    session.save(doc, Some(&copied)).unwrap();
    let saved = files::load_delimitation(&copied).unwrap();
    assert_eq!(
        basecraft_ops::delimitation::checked_alignment(&saved, &copied)
            .unwrap()
            .unwrap(),
        linked
    );
    let package = root.path().join("package");
    provenance::package(&copied, &package).unwrap();
    let exported = package.join("copied.spart");
    let exported_work = files::load_delimitation(&exported).unwrap();
    assert!(provenance::verify(&exported).unwrap().is_empty());
    assert_eq!(
        basecraft_io::hash_file(
            &basecraft_ops::delimitation::checked_alignment(&exported_work, &exported)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        saved.exchange_fields["basecraft_alignment_hash"]
    );
    std::fs::write(&linked, "damaged preserved alignment").unwrap();
    assert!(basecraft_ops::delimitation::checked_alignment(&saved, &copied).is_err());
}

#[test]
fn combine_complete_export_includes_legacy_sidecar_evidence() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let text = include_str!("../../../testdata/fixtures/delimitation/manual.spart");
    let source = root.join("legacy.spart");
    let target = root.join("target.spart");
    std::fs::write(&source, text).unwrap();
    std::fs::write(&target, text).unwrap();
    let mut legacy = basecraft_io::spart::parse_text(text).unwrap();
    legacy.exchange_fields.insert(
        "authored_metadata".into(),
        "Distinct original sidecar context".into(),
    );
    legacy.hypotheses[0].provenance.parameters.insert(
        "reviewed_method_context".into(),
        json!("Sidecar-only scientific context"),
    );
    let mut bytes = Vec::new();
    files::write_delimitation_sidecar(
        &legacy,
        &basecraft_io::hash_file(&source).unwrap(),
        &mut bytes,
    )
    .unwrap();
    std::fs::write(files::tree_sidecar(&source), &bytes).unwrap();
    let side_hash = basecraft_io::hash_bytes(&bytes);
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&target).unwrap();
    basecraft_ops::delimitation::combine(&mut session, doc, &[source]).unwrap();
    let combined = root.join("combined.spart");
    session.save(doc, Some(&combined)).unwrap();
    let package = parent.path().join("package");
    provenance::package(&combined, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let exported = package.join("combined.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let artifacts = provenance::companion(&exported).join("artifacts");
    assert!(
        std::fs::read_dir(&artifacts)
            .unwrap()
            .any(|entry| basecraft_io::hash_file(&entry.unwrap().path()).unwrap() == side_hash),
        "complete export omitted the legacy sidecar whose hash Combine declares: {side_hash}"
    );
}

#[test]
fn combine_discovers_legacy_provenance_and_guards_sidecar_changes() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy.spart");
    std::fs::write(
        &source,
        include_str!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let work = files::load_delimitation(&source).unwrap();
    let mut side = Vec::new();
    files::write_delimitation_sidecar(&work, &basecraft_io::hash_file(&source).unwrap(), &mut side)
        .unwrap();
    let sidecar = files::tree_sidecar(&source);
    std::fs::write(&sidecar, &side).unwrap();
    let side_hash = basecraft_io::hash_bytes(&side);
    let legacy_bundle = provenance::companion(&source);
    std::fs::create_dir_all(legacy_bundle.join("records")).unwrap();
    std::fs::create_dir(legacy_bundle.join("artifacts")).unwrap();
    let context = b"Distinct retained legacy methods evidence";
    let context_hash = basecraft_io::hash_bytes(context);
    let retained = format!("artifacts/{}", context_hash.trim_start_matches("b3:"));
    std::fs::write(legacy_bundle.join(&retained), context).unwrap();
    let record = json!({
        "schema_version":1, "id":"legacy", "session":"legacy", "parents":[], "parent_hashes":{},
        "created":"2026-10-16T00:00:00Z", "origin":"Legacy source publication",
        "inputs":[{"id":"legacy:context", "logical_name":"methods.txt", "hash":context_hash, "size":context.len(), "retained":retained}],
        "outputs":[], "events":[], "effective_edits":[],
        "completeness":{"ancestry_known":false,"input_bytes_available":true,"execution_specification_available":false,"replay_verified":false},
        "report_template_version":2
    });
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    std::fs::write(legacy_bundle.join("manifest.json"), &bytes).unwrap();
    std::fs::write(legacy_bundle.join("records/legacy.json"), &bytes).unwrap();
    let target = root.path().join("combined.spart");
    std::fs::copy(&source, &target).unwrap();
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&target).unwrap();
    basecraft_ops::delimitation::combine(&mut session, doc, std::slice::from_ref(&source)).unwrap();
    session.save(doc, None).unwrap();
    let conflict = conflicts(&sidecar).unwrap();
    assert!(conflict.analyses.iter().any(|a| a.direct));
    assert!(conflict.analyses.iter().any(|a| !a.direct));
    std::fs::write(&sidecar, "changed").unwrap();
    assert!(!provenance::verify(&target).unwrap().is_empty());
    assert!(provenance::package(&target, &root.path().join("rejected")).is_err());
    std::fs::write(&sidecar, &side).unwrap();
    // The original decision remains valid only because preservation rechecks
    // the exact accepted bytes; a fresh stat-only offer would reject the rewrite.
    resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::remove_file(&sidecar).unwrap();
    let package = root.path().join("complete");
    provenance::package(&target, &package).unwrap();
    std::fs::remove_file(&source).unwrap();
    std::fs::remove_dir_all(&legacy_bundle).unwrap();
    let exported = package.join("combined.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let hashes: Vec<_> = std::fs::read_dir(provenance::companion(&exported).join("artifacts"))
        .unwrap()
        .map(|entry| basecraft_io::hash_file(&entry.unwrap().path()).unwrap())
        .collect();
    assert!(hashes.contains(&context_hash));
    assert!(hashes.contains(&side_hash));
}

#[test]
#[cfg(unix)]
fn species_partial_save_retains_recoverable_edits_and_retries() {
    use basecraft_protocol::{Op, delimitation::DelimitationEdit};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let canonical = root.path().join("species.spart");
    std::fs::write(
        &canonical,
        include_str!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&canonical).unwrap();
    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Derive {
                    source: "partition-1".into(),
                    id: "editable".into(),
                    name: "Original".into(),
                }),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Rename {
                    hypothesis: "editable".into(),
                    name: "Pending edit".into(),
                }),
            },
        )
        .unwrap();
    let sidecar = files::tree_sidecar(&canonical);
    let directory = sidecar.parent().unwrap();
    let permissions = std::fs::metadata(directory).unwrap().permissions();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = session.save(doc, None);
    std::fs::set_permissions(directory, permissions).unwrap();
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("data saved") && error.contains("incomplete"),
        "{error}"
    );
    assert!(session.recovery_root().join("pending-save.json").is_file());
    let recovered = root.path().join("recovered");
    basecraft_ops::replay::recover(session.recovery_root(), &recovered).unwrap();
    let work = files::load_delimitation(&recovered.join("species.spart")).unwrap();
    assert_eq!(work.hypotheses.last().unwrap().name, "Pending edit");
    session.save(doc, None).unwrap();
    assert_eq!(
        files::load_delimitation(&canonical)
            .unwrap()
            .hypotheses
            .last()
            .unwrap()
            .name,
        "Pending edit"
    );
    assert!(provenance::verify(&canonical).unwrap().is_empty());
}
