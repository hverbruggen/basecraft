//! The GUI annotation route: a registered sequence operation run against an
//! open document through the shared job runner, skipping known features.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, provenance};
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
            "generation": session.generation(doc),
            "revision": session.checked_sequence(doc).unwrap().version()}]},
        "parameters": {"min_codons": 100}}))
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
    let run = basecraft_ops::analysis_store::ArtifactRun::runs_for_anchor(&file, 64)
        .unwrap()
        .into_iter()
        .find(|run| run.ends_with(&first.run_id))
        .unwrap();
    assert!(run.join("snapshot.arrow-pair").is_file());
    assert!(run.join("runtime.json").is_file());

    session.save(doc, None).unwrap();
    // FASTA keeps its visible scientific GFF3 pair.
    assert!(basecraft_ops::files::gff_sidecar(&file).is_file());
    drop(session);
    let publication = provenance::inspect(&file).unwrap();
    assert!(publication.events.iter().any(|event| matches!(&event.op,
        Op::ToolRun { params, .. }
            if params["effective"]["min_codons"] == 100 && params["initiator"] == "GUI"
                && params["run_id"] == first.run_id.as_str())));
    assert_eq!(provenance::verify(&file).unwrap(), Vec::<String>::new());

    // Moving the whole folder keeps the run and the saved record together.
    let moved = temp.path().join("moved");
    std::fs::rename(&project, &moved).unwrap();
    let file = moved.join("orf.fasta");
    assert_eq!(provenance::verify(&file).unwrap(), Vec::<String>::new());
    let reopened = basecraft_ops::files::load(&file).unwrap();
    let basecraft_ops::files::Loaded::Sequence(reopened) = reopened else {
        panic!("not a sequence")
    };
    assert_eq!(reopened.doc.feature_count(), 1);
}
