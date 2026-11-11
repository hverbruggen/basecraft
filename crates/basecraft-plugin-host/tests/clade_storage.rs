//! Clade selection uses the same generated/document input and publication path
//! as the GUI and headless registered operation routes.
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

#[test]
fn unsaved_clade_tree_with_referenced_alignment_retains_both_inputs_and_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let alignment = root.join("source.afa");
    let tree = root.join("tree.nwk");
    std::fs::write(&alignment, ">a\nAC-G\n>b\nAT-G\n>c\nTTTT\n").unwrap();
    std::fs::write(&tree, "((a,b),c);\n").unwrap();
    let registry = Registry::load(&RegistryConfig {
        bundled: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        packages: root.join("profile/plugins"),
        index: root.join("profile/index.json"),
    })
    .unwrap();
    let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap();
    let mut session = Session::recoverable_with_paths(root, &paths).unwrap();
    let doc = session.open(&tree).unwrap();
    session
        .apply(doc, Op::TreeLadderize { small_first: true })
        .unwrap();
    let expected = basecraft_io::hash_bytes(session.tree(doc).unwrap().to_newick().as_bytes());
    assert!(session.saved_source(doc).unwrap().is_none());
    let request: OperationRequest = serde_json::from_value(json!({
        "operation":"basecraft.clade-alignment/extract@1",
        "inputs":{"alignment":[{"kind":"file","path":"source.afa"}],
            "selection_tree":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":session.revision(doc)}]},
        "parameters":{"taxa":["a","b"]},
        "destinations":{"artifacts/clade.afa":"selected.afa"}
    })).unwrap();
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit_in_session(
            &registry,
            request,
            SessionPolicy::new(root, true).unwrap(),
            "GUI",
            &mut session,
        )
        .unwrap();
    // The current document can change after preparation. The result must still
    // cite the submitted selection revision, not this newer one.
    session
        .apply(
            doc,
            Op::TreeRenameTip {
                from: "c".into(),
                to: "new-c".into(),
            },
        )
        .unwrap();
    let outcome = jobs
        .wait_in_session(&accepted.run_id, &mut session, &|| false)
        .unwrap();
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    let result = root.join("selected.afa");
    assert_eq!(
        std::fs::read_to_string(&result).unwrap(),
        ">a\nAC-G\n>b\nAT-G\n"
    );
    let publication = records::inspect(&result).unwrap();
    assert!(
        publication
            .inputs
            .iter()
            .any(|a| a.hash == expected && a.retained.is_some())
    );
    assert!(
        publication.inputs.iter().any(
            |a| a.reference.is_some() && a.hash == basecraft_io::hash_file(&alignment).unwrap()
        )
    );
    assert!(
        !publication
            .events
            .iter()
            .any(|e| matches!(e.op, Op::TreeRenameTip { .. }))
    );
    assert!(publication.events.iter().any(|e| matches!(&e.op, Op::ToolRun { params, .. } if params.pointer("/effective/taxa") == Some(&json!(["a","b"])))));
    assert_eq!(records::verify(&result).unwrap(), Vec::<String>::new());
    let package = root.join("package");
    records::package(&result, &package).unwrap();
    std::fs::remove_file(&alignment).unwrap();
    assert_eq!(
        records::verify(&package.join("selected.afa")).unwrap(),
        Vec::<String>::new()
    );
}
