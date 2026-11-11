//! Registered distance operations: outputs, provenance, history and replay.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::AlignmentLinkExt;
mod common;

use std::path::Path;

use basecraft_ops::Session;
use serde_json::{Value, json};

/// The distance engine the plugin runs; the test environment names it.
fn engine() -> bool {
    let found = std::env::var_os("BASECRAFT_DISTANCE").is_some();
    if !found {
        eprintln!("set BASECRAFT_DISTANCE to exercise the distance plugin");
    }
    found
}

fn tree_request(
    session: &Session,
    doc: basecraft_ops::DocId,
    parameters: &Value,
    destination: &str,
) -> Value {
    json!({"operation":"basecraft.distance/tree@1","inputs":{"alignment":common::document(session, doc)},
        "parameters":parameters,"destinations":{"artifacts/tree.nwk":destination}})
}

#[test]
fn distance_outputs_are_logged_and_replay() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = root.join("input.aln.fasta");
    std::fs::write(
        &input,
        ">a\nAAAAAAAC\n>b\nAAAAAACC\n>c\nCCAAAAAC\n>d\nCCAAAACC\n",
    )
    .unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    let registry = common::bundled();
    let parameters = json!({"model":"jc69","threads":2});
    let outcome = common::run_in(&registry, &mut session, |s| {
        tree_request(s, doc, &parameters, "tree.nwk")
    });
    common::assert_completed(&outcome);
    let output = root.join("tree.nwk");
    let tree = session.open(&output).unwrap();
    assert_eq!(session.tree(tree).unwrap().tip_count(), 4);
    // The run report and distance data are retained members, never visible.
    common::member(&outcome, "tree.nwk.run.json");
    assert_eq!(common::visible(root), ["input.aln.fasta", "tree.nwk"]);
    let runs = common::tool_runs(&session, doc);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["effective"]["model"], "jc69");
    assert_eq!(runs[0]["run_id"], outcome.run_id.as_str());

    let parameters = json!({"model":"jc69","threads":2,"method":"upgma"});
    let upgma = common::run_in(&registry, &mut session, |s| {
        tree_request(s, doc, &parameters, "upgma.nwk")
    });
    common::assert_completed(&upgma);
    let metadata = common::read_json(&common::member(&upgma, "tree.nwk.run.json"));
    assert_eq!(metadata["method"], "upgma");
    let tree = session.open(&root.join("upgma.nwk")).unwrap();
    assert_eq!(session.tree(tree).unwrap().tip_count(), 4);
    drop(session);
    let replay = basecraft_ops::records::replay(root).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
}

#[test]
fn forest_members_share_one_alignment_snapshot() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("forest.afa");
    let original = ">a\nAAA---\n>b\nAAT---\n>c\n---CCC\n>d\n---CCT\n";
    std::fs::write(&input, original).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&input).unwrap();
    let registry = common::bundled();
    let parameters = json!({"model":"p"});
    // A result never replaces its own input.
    let refused = common::refused_in(&registry, &mut session, |s| {
        tree_request(s, doc, &parameters, "forest.afa")
    });
    assert!(refused.contains("output_conflict"), "{refused}");
    assert_eq!(std::fs::read_to_string(&input).unwrap(), original);
    let outcome = common::run_in(&registry, &mut session, |s| {
        tree_request(s, doc, &parameters, "forest.nwk")
    });
    common::assert_completed(&outcome);
    let output = dir.path().join("forest.nwk");
    let main = basecraft_ops::records::AlignmentLink::read(&output).unwrap();
    // Component trees travel with the exported tree and share its snapshot.
    for index in 1..=2 {
        let path = dir.path().join(format!("forest.component-{index}.nwk"));
        let tree = session.open(&path).unwrap();
        let link = basecraft_ops::records::AlignmentLink::read(&path).unwrap();
        assert_eq!(link.snapshot, main.snapshot);
        assert!(
            link.checked_snapshot(&path, session.tree(tree).unwrap())
                .is_ok()
        );
    }
    assert!(!dir.path().join("forest.component-3.nwk").exists());
    assert!(
        !common::visible(dir.path())
            .iter()
            .any(|name| name.ends_with(".alignment.afa"))
    );
    assert!(basecraft_ops::records::replay(dir.path()).unwrap().ok());
}

#[test]
fn bootstrap_provenance_success_and_withheld_support_replay() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let registry = common::bundled();
    for (case, input_text) in [
        (
            "complete",
            ">a\nAAAAAAAAAACCAAAACCAA\n>b\nAAAAAAAAAACCAAAACCAA\n>c\nAAAAGGAAAACCAAAACCAA\n>d\nAAAAGGAAAACCAAAACCAA\n",
        ),
        ("gaps", ">a\nA---\n>b\nAAAA\n"),
    ] {
        let input = root.join(format!("{case}.afa"));
        std::fs::write(&input, input_text).unwrap();
        let mut session = Session::new(root);
        let doc = session.open(&input).unwrap();
        for method in ["sorted", "upgma"] {
            let name = format!("{case}-{method}.nwk");
            let output = root.join(&name);
            let parameters =
                json!({"model":"p","method":method,"bootstrap":64,"seed":71,"threads":2});
            let outcome = common::run_in(&registry, &mut session, |s| {
                tree_request(s, doc, &parameters, &name)
            });
            common::assert_completed(&outcome);
            let report = common::read_json(&common::member(&outcome, "tree.nwk.run.json"));
            assert_eq!(report["bootstrap"]["requested"], 64);
            assert_eq!(report["bootstrap"]["seed"], 71);
            assert_eq!(report["bootstrap"]["method"], method);
            assert_eq!(report["bootstrap"]["support_withheld"], case == "gaps");
            if case == "complete" {
                assert_eq!(report["bootstrap"]["completed"], 64);
                let tree = session.open(&output).unwrap();
                let generated = session.tree(tree).unwrap();
                assert_eq!(generated.supports()[0].name, "Bootstrap");
                assert_eq!(
                    generated.supports()[0].kind,
                    basecraft_protocol::SupportKind::Bootstrap
                );
                assert_eq!(generated.supports()[0].decimals(generated.style()), 0);
                let link = basecraft_ops::records::AlignmentLink::read(&output).unwrap();
                assert!(link.inferred);
                assert_eq!(link.source, input);
                assert!(link.checked_snapshot(&output, generated).is_ok());
                session.save(tree, None).unwrap();
                let reopened = basecraft_ops::files::load_tree(&output).unwrap().doc;
                assert_eq!(reopened.supports()[0].name, "Bootstrap");
                assert_eq!(
                    reopened.supports()[0].kind,
                    basecraft_protocol::SupportKind::Bootstrap
                );
            } else {
                // Withheld support leaves a tree without invented support values.
                let tree = basecraft_ops::files::load_tree(&output).unwrap().doc;
                assert_eq!(tree.supports().len(), 0);
            }
            let runs = common::tool_runs(&session, doc);
            let run = runs
                .iter()
                .find(|run| run["run_id"] == outcome.run_id.as_str())
                .unwrap();
            assert_eq!(run["effective"]["bootstrap"], 64);
            assert_eq!(run["effective"]["seed"], 71);
        }
    }
    let replay = basecraft_ops::records::replay(root).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
}

#[test]
fn reference_tree_save_as_and_complete_export_retain_alignment_and_support() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.afa");
    std::fs::write(
        &source,
        ">a\nAAAAAACCAA\n>b\nAAAAAACCAA\n>c\nAAGGAACCAA\n>d\nAAGGAACCAA\n",
    )
    .unwrap();
    let output = root.path().join("tree.nwk");
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    let parameters = json!({"model":"p","method":"sorted","bootstrap":8,"threads":1});
    let outcome = common::run_in(&common::bundled(), &mut session, |s| {
        tree_request(s, doc, &parameters, "tree.nwk")
    });
    common::assert_completed(&outcome);
    let tree = session.open(&output).unwrap();
    let copied = root.path().join("copied.nwk");
    session.save(tree, Some(&copied)).unwrap();
    let link = basecraft_ops::records::AlignmentLink::read(&copied).unwrap();
    assert_eq!(link.schema, 2);
    assert!(
        link.checked_snapshot(&copied, session.tree(tree).unwrap())
            .is_ok()
    );
    assert!(
        !common::visible(root.path())
            .iter()
            .any(|name| name.ends_with(".alignment.afa"))
    );
    let package = root.path().join("portable");
    basecraft_ops::records::package(&output, &package).unwrap();
    std::fs::remove_file(&source).unwrap();
    let exported = package.join("tree.nwk");
    assert_eq!(
        basecraft_ops::records::verify(&exported).unwrap(),
        Vec::<String>::new()
    );
    let tree = basecraft_ops::files::load_tree(&exported).unwrap().doc;
    assert_eq!(
        tree.supports()[0].kind,
        basecraft_protocol::SupportKind::Bootstrap
    );
    let link = basecraft_ops::records::AlignmentLink::read(&exported).unwrap();
    assert!(link.checked_snapshot(&exported, &tree).is_ok());
    assert!(Path::new(&common::evidence(&outcome)).is_dir());
}

#[test]
fn registered_cache_reuses_distances_across_consumers_and_refuses_mismatch() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("input.afa");
    std::fs::write(&source, ">a\nAAAAAC\n>b\nAAAACC\n>c\nAAACCC\n").unwrap();
    let registry = common::bundled();
    let matrix = common::run(
        &registry,
        root,
        json!({"operation":"basecraft.distance/matrix@1",
        "inputs":{"alignment":common::file("input.afa")},"parameters":{"model":"p"},
        "destinations":{"artifacts/distances.tsv.distances.bin":"cache.json"}}),
    );
    common::assert_completed(&matrix);
    assert_eq!(
        common::read_json(&common::member(&matrix, "distances.tsv.run.json"))["cache_reused"],
        false
    );
    let cached = std::fs::read(root.join("cache.json")).unwrap();
    for operation in ["tree", "matrix", "groups"] {
        let mut parameters = json!({"model":"p"});
        if operation == "groups" {
            parameters["groups_data"] = json!({"a":"one","b":"one","c":"two"});
        }
        let outcome = common::run(
            &registry,
            root,
            json!({"operation":format!("basecraft.distance/{operation}@1"),
            "inputs":{"alignment":common::file("input.afa"),"cache":common::file("cache.json")},"parameters":parameters}),
        );
        common::assert_completed(&outcome);
        let report = match operation {
            "tree" => "tree.nwk.run.json",
            "matrix" => "distances.tsv.run.json",
            _ => "groups.tsv.run.json",
        };
        assert_eq!(
            common::read_json(&common::member(&outcome, report))["cache_reused"],
            true
        );
        if operation == "matrix" {
            assert_eq!(
                std::fs::read(common::member(&outcome, "distances.tsv")).unwrap(),
                std::fs::read(common::member(&matrix, "distances.tsv")).unwrap()
            );
        }
        assert_eq!(
            basecraft_ops::records::verify(&common::member(&outcome, report)).unwrap(),
            Vec::<String>::new()
        );
    }
    assert_eq!(std::fs::read(root.join("cache.json")).unwrap(), cached);
    for parameters in [
        json!({"model":"jc69"}),
        json!({"model":"p","min_overlap":2}),
    ] {
        let outcome = common::run(
            &registry,
            root,
            json!({"operation":"basecraft.distance/tree@1",
            "inputs":{"alignment":common::file("input.afa"),"cache":common::file("cache.json")},"parameters":parameters}),
        );
        assert_eq!(
            outcome.state,
            basecraft_protocol::operation::RuntimeState::Failed
        );
        assert!(
            outcome
                .errors
                .iter()
                .any(|e| e.message.contains("do not match"))
        );
    }
    // Publication verification refuses a changed cache before accepting a run.
    std::fs::write(root.join("cache.json"), "{}").unwrap();
    let mut manager = basecraft_plugin_host::jobs::JobManager::default();
    assert!(manager.submit(&registry, serde_json::from_value(json!({"operation":"basecraft.distance/tree@1",
        "inputs":{"alignment":common::file("input.afa"),"cache":common::file("cache.json")},"parameters":{"model":"p"}})).unwrap(),
        basecraft_plugin_host::policy::SessionPolicy::new(root,true).unwrap(), "test").is_err());
}

#[test]
fn bootstrap_destination_publishes_working_copy_and_repairs_late_collision() {
    crate::isolated_profile::isolate!();
    if !engine() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("input.afa"),
        ">a\nAAAAAC\n>b\nAAAACC\n>c\nAAACCC\n",
    )
    .unwrap();
    let registry = common::bundled();
    for destination in ["metadata", "working"] {
        let outcome = common::run(
            &registry,
            root.path(),
            json!({"operation":"basecraft.distance/tree@1",
            "inputs":{"alignment":common::file("input.afa")},
            "parameters":{"model":"p","method":"sorted","bootstrap":16,"bootstrap_destination":destination}}),
        );
        common::assert_completed(&outcome);
        let name = format!("bootstrap-{}.nex", outcome.run_id);
        let retained = common::member(&outcome, &name);
        let visible = root.path().join(&name);
        assert_eq!(visible.exists(), destination == "working");
        if visible.exists() {
            assert_eq!(
                std::fs::read(&visible).unwrap(),
                std::fs::read(retained).unwrap()
            );
            assert_eq!(
                basecraft_ops::records::verify(&visible).unwrap(),
                Vec::<String>::new()
            );
        }
    }
    let mut manager = basecraft_plugin_host::jobs::JobManager::default();
    let accepted = manager.submit(&registry, serde_json::from_value(json!({"operation":"basecraft.distance/tree@1",
        "inputs":{"alignment":common::file("input.afa")},
        "parameters":{"model":"p","method":"sorted","bootstrap":256,"bootstrap_destination":"working"}})).unwrap(),
        basecraft_plugin_host::policy::SessionPolicy::new(root.path(),true).unwrap(), "test").unwrap();
    let visible = root
        .path()
        .join(format!("bootstrap-{}.nex", accepted.run_id));
    std::fs::write(&visible, "foreign").unwrap();
    let partial = common::wait(&manager, &accepted.run_id);
    assert_eq!(
        partial.state,
        basecraft_protocol::operation::RuntimeState::Partial,
        "{partial:#?}"
    );
    assert_eq!(std::fs::read_to_string(&visible).unwrap(), "foreign");
    std::fs::remove_file(&visible).unwrap();
    common::assert_completed(&manager.repair(&accepted.run_id).unwrap());
    assert_eq!(
        basecraft_ops::records::verify(&visible).unwrap(),
        Vec::<String>::new()
    );
}
