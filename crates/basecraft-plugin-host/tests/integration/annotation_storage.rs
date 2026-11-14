//! The GUI annotation route: a registered sequence operation run against an
//! open document through the shared job runner, skipping known features.
use basecraft_ops::{Session, records};
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::{
    Op,
    operation::{OperationRequest, RuntimeState},
};
use serde_json::json;
use std::path::Path;

fn python_ready() -> bool {
    let python = basecraft_plugin_host::process::python_interpreter();
    let ok = std::process::Command::new(&python)
        .args(["-c", "import pyarrow, numpy"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: {} lacks pyarrow or numpy", python.display());
    }
    ok
}

fn run(
    jobs: &mut JobManager,
    registry: &Registry,
    session: &mut Session,
    doc: basecraft_ops::DocId,
) -> basecraft_protocol::operation::OperationOutcome {
    let request: OperationRequest = serde_json::from_value(json!({
        "operation": "basecraft.orf-finder/find_orfs@1",
        "inputs": {"sequence": [{"kind": "document", "doc": doc.0.to_string(),
            "generation": session.generation(doc).unwrap(),
            "revision": session.checked_sequence(doc).unwrap().version()}]},
        "parameters": {"min_codons": 100, "genetic_code": 11}}))
    .unwrap();
    let policy = SessionPolicy::new(session.root(), true).unwrap();
    let accepted = jobs
        .submit_in_session(registry, request, policy, "GUI", session)
        .unwrap();
    jobs.wait_in_session(&accepted.run_id, session, &|| false)
        .unwrap()
}

#[test]
fn gui_annotation_keeps_its_invocation_in_the_hidden_store_and_skips_known_features() {
    crate::isolated_profile::isolate!();
    if !python_ready() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let config = RegistryConfig {
        bundled: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        packages: temp.path().join("profile/plugins"),
        index: temp.path().join("profile/index.json"),
    };
    let registry = Registry::load(&config).unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join("orf.fasta");
    let mut seq = "C".repeat(10);
    seq.push_str("ATG");
    seq.push_str(&"AAA".repeat(120));
    seq.push_str("TAA");
    seq.push_str(&"C".repeat(200));
    std::fs::write(&file, format!(">orf\n{seq}\n")).unwrap();
    let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| temp.path().join("profile").into_os_string())
    })
    .unwrap();
    let mut session = Session::recoverable_with_paths(&project, &paths).unwrap();
    let doc = session.open(&file).unwrap();
    let mut jobs = JobManager::default().skipping_known_features();

    let first = run(&mut jobs, &registry, &mut session, doc);
    assert_eq!(first.state, RuntimeState::Completed, "{first:?}");
    let proposals = first.proposals.clone().unwrap();
    assert_eq!((proposals.applied, proposals.rejected.len()), (1, 0));
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
    // A rerun proposes the same ORF: reported as known, nothing added.
    let second = run(&mut jobs, &registry, &mut session, doc);
    assert_eq!(second.state, RuntimeState::Completed, "{second:?}");
    let proposals = second.proposals.unwrap();
    assert_eq!((proposals.applied, proposals.rejected.len()), (0, 1));
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
    // The applied proposals are one undo step.
    session.undo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().feature_count(), 0);
    session.redo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);

    // The submitted snapshot and request live in the hidden run, not beside the file.
    let evidence = Path::new(first.evidence.as_ref().unwrap());
    assert!(evidence.starts_with(project.join(".basecraft").canonicalize().unwrap()));
    let run = basecraft_ops::records::ArtifactRun::runs_for_anchor(&file, 64)
        .unwrap()
        .into_iter()
        .find(|run| run.ends_with(&first.run_id))
        .unwrap();
    assert!(run.join("snapshot.arrow-pair").is_file());
    assert!(run.join("runtime.json").is_file());

    // Methods names the genetic code the ORFs were read with.
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(
        report.methods.contains("genetic_code = 11"),
        "{}",
        report.methods
    );

    session.save(doc, None).unwrap();
    // FASTA keeps its visible scientific GFF3 pair.
    assert!(basecraft_ops::files::gff_sidecar(&file).is_file());
    drop(session);
    let publication = records::inspect(&file, None).unwrap();
    // The entry names its run; the run record holds the effective settings.
    let link = publication
        .events
        .iter()
        .find_map(|event| match &event.op {
            Op::ToolRun { run: Some(run), .. } if run.id == first.run_id => Some(run.clone()),
            _ => None,
        })
        .unwrap();
    let record: basecraft_protocol::records::RunRecord = serde_json::from_slice(
        &std::fs::read(project.join(&link.folder).join("execution.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record.effective_parameters["min_codons"], 100);
    assert_eq!(record.initiator, "GUI");
    assert_eq!(records::verify(&file, None).unwrap(), Vec::<String>::new());

    // Moving the whole folder keeps the run and the saved record together.
    let moved = temp.path().join("moved");
    std::fs::rename(&project, &moved).unwrap();
    let file = moved.join("orf.fasta");
    assert_eq!(records::verify(&file, None).unwrap(), Vec::<String>::new());
    let reopened = basecraft_ops::files::load(&file).unwrap();
    let basecraft_ops::files::Loaded::Sequence(reopened) = reopened else {
        panic!("not a sequence")
    };
    assert_eq!(reopened.doc.feature_count(), 1);
}

/// Finding ORFs and predicting genes have no default genetic code: a
/// request without one is refused before the plugin starts, on every route
/// that submits it.
#[test]
fn annotation_requests_without_a_genetic_code_are_refused() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let registry = Registry::load(&RegistryConfig {
        bundled: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        packages: temp.path().join("profile/plugins"),
        index: temp.path().join("profile/index.json"),
    })
    .unwrap();
    let file = temp.path().join("orf.fasta");
    std::fs::write(&file, ">orf\nATGAAATAA\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&file).unwrap();
    for (operation, parameters, missing) in [
        (
            "basecraft.orf-finder/find_orfs@1",
            json!({"min_codons": 1}),
            "needs the setting genetic_code",
        ),
        (
            "basecraft.pyrodigal/predict_genes@1",
            json!({}),
            "needs the setting translation_table",
        ),
    ] {
        let request: OperationRequest = serde_json::from_value(json!({
            "operation": operation,
            "inputs": {"sequence": [{"kind": "document", "doc": doc.0.to_string(),
                "generation": session.generation(doc).unwrap(),
                "revision": session.checked_sequence(doc).unwrap().version()}]},
            "parameters": parameters}))
        .unwrap();
        let policy = SessionPolicy::new(session.root(), true).unwrap();
        let error = JobManager::default()
            .submit_in_session(&registry, request, policy, "GUI", &mut session)
            .unwrap_err();
        assert!(error.to_string().contains(missing), "{error}");
    }
    assert_eq!(session.doc(doc).unwrap().feature_count(), 0);
}
