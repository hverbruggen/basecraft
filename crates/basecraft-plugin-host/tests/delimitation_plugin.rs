//! Actual public socket/SDK/native-engine workflows through registered
//! operations; no mock numerical results. Cancellation uses a gated engine
//! wrapper to hold execution without relying on scientific workload duration.
use basecraft_ops::records::AlignmentLinkExt;
mod common;
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{Session, input_evidence, records};
use basecraft_plugin_host::jobs::JobManager;
use basecraft_protocol::{operation::RuntimeState, records::JobStatus};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The test environment names the native delimitation engine the plugin runs.
fn require_engine() {
    let engine = std::env::var_os("BASECRAFT_DELIMITATION")
        .expect("set BASECRAFT_DELIMITATION to the built basecraft-delimitation");
    assert!(
        Path::new(&engine).is_file(),
        "Build basecraft-delimitation before this integration test"
    );
}

/// The first file called `name` below `directory`.
fn find(directory: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(directory).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|file| file == name) {
            return Some(path);
        }
    }
    None
}

/// Statuses of the runs the session recorded as finished for `doc`.
fn finished(session: &Session, doc: basecraft_ops::DocId) -> Vec<(String, JobStatus)> {
    session
        .document_record(doc)
        .unwrap()
        .events
        .iter()
        .cloned()
        .filter_map(|event| match event.op {
            basecraft_protocol::Op::JobFinished { report } => Some((report.id, report.status)),
            _ => None,
        })
        .collect()
}

#[test]
fn snapshots_and_terminal_outcomes_survive_success_and_failure() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tree.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let mut expected = Vec::new();
    for (name, rooted) in [("success", true), ("failed", false)] {
        let workspace = format!("{name}.spart");
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":"basecraft.delimitation/gmyc_single@1",
                "inputs":{"tree":common::document(s, doc)},
                "parameters":{"rooted":rooted},"destinations":{"workspace":workspace}})
        });
        // The submitted tree is referenced in place, with its accepted hash.
        let run = common::evidence(&outcome);
        let input = input_evidence::read_input(&run).unwrap();
        assert_eq!(
            input.state,
            basecraft_protocol::input_records::EvidenceState::Referenced
        );
        assert_eq!(
            input.source.unwrap().hash,
            basecraft_io::hash_file(&source).unwrap()
        );
        assert!(run.join("execution.json").is_file());
        assert!(!root.path().join(format!("{name}.json")).exists());
        if rooted {
            common::assert_completed(&outcome);
            let published = root.path().join(&workspace);
            assert!(published.is_file());
            assert_eq!(common::artifact(&outcome, &workspace), published);
            expected.push((outcome.run_id.clone(), JobStatus::Completed));
        } else {
            assert_eq!(outcome.state, RuntimeState::Failed, "{outcome:#?}");
            assert!(!root.path().join(&workspace).exists());
            expected.push((outcome.run_id.clone(), JobStatus::Failed));
        }
    }
    // Both attempts are in the tree's history with their terminal state.
    assert_eq!(finished(&session, doc), expected);
    drop(session);
    let report = records::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn exact_identity_precedes_public_tree_inference_and_restores_specimens() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("aligned.fasta");
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
    let registry = common::bundled();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    for (unique, name) in [(true, "unique"), (false, "all")] {
        let workspace = format!("{name}.spart");
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":"basecraft.host/gmyc_alignment_single@1",
                "inputs":{"alignment":common::document(s, doc)},
                "parameters":{"unique_haplotypes":unique,"model":"p"},
                "destinations":{"workspace":workspace}})
        });
        common::assert_completed(&outcome);
        assert!(root.path().join(&workspace).is_file());
        let stages: Vec<basecraft_protocol::operation::OperationOutcome> = serde_json::from_value(
            common::read_json(&common::evidence(&outcome).join("stages.json")),
        )
        .unwrap();
        if std::env::var_os("BASECRAFT_CAPTURE_WORKFLOW_RECORDS").is_some() {
            let fixtures =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
            for (file, name) in [
                ("stages.json", "workflow-stages.json"),
                ("validated-artifacts.json", "validated-artifacts.json"),
            ] {
                std::fs::copy(common::evidence(&outcome).join(file), fixtures.join(name)).unwrap();
            }
        }
        assert_eq!(stages.len(), 3);
        for stage in &stages {
            common::assert_completed(stage);
            assert!(common::evidence(stage).join("execution.json").is_file());
        }
        let publication = basecraft_ops::records::inspect(&root.path().join(&workspace)).unwrap();
        for operation in ["haplotypes", "tree", "gmyc_single"] {
            assert!(
                publication.events.iter().any(|event| matches!(&event.op,
                basecraft_protocol::Op::ToolRun { operation: actual, .. } if actual == operation)),
                "missing stage {operation}"
            );
        }
        let result = common::read_json(&common::artifact(&outcome, "fit.json"));
        assert_eq!(
            result["workspace"]["specimens"].as_array().unwrap().len(),
            5
        );
        if unique {
            assert_eq!(
                result["workspace"]["hypotheses"][0]["provenance"]["haplotypes"]["a"],
                json!(["a", "b"])
            );
            assert_eq!(
                result["workspace"]["hypotheses"][0]["provenance"]["parameters"]["retained_count"],
                4
            );
            let memberships: Vec<_> = result["workspace"]["hypotheses"][0]["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|g| g["members"].as_array().unwrap())
                .collect();
            assert!(
                memberships
                    .iter()
                    .any(|members| members.contains(&json!("a")) && members.contains(&json!("b")))
            );
        } else {
            // Every specimen is a tip, so identical a and b meet at age zero;
            // they are fitted as one tip and both come back in one species.
            assert_eq!(
                result["metrics"]["zero_distance_groups"]["a"],
                json!(["a", "b"])
            );
            assert!(
                result["job"]["diagnostics"]
                    .to_string()
                    .contains("zero distance")
            );
            assert!(
                result["workspace"]["hypotheses"][0]["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|g| g["members"] == json!(["a", "b"])
                        || (g["members"].as_array().unwrap().contains(&json!("a"))
                            && g["members"].as_array().unwrap().contains(&json!("b"))))
            );
        }
    }
    // Two identical sequences leave one haplotype: no tree or fit is invented.
    let single = root.path().join("single.fasta");
    std::fs::write(&single, ">a\nACGT\n>b\nACGT\n").unwrap();
    let single_doc = session.open(&single).unwrap();
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.host/gmyc_alignment_single@1",
            "inputs":{"alignment":common::document(s, single_doc)},
            "parameters":{"unique_haplotypes":true},"destinations":{"workspace":"single.spart"}})
    });
    assert_eq!(outcome.state, RuntimeState::Failed, "{outcome:#?}");
    assert!(
        outcome.errors[0]
            .message
            .contains("fewer than two retained haplotypes"),
        "{outcome:#?}"
    );
    let report = common::read_json(
        &find(&common::evidence(&outcome), "fit.json.run.json").expect("unavailable report"),
    );
    assert_eq!(report["job"]["status"], "unavailable");
    let bytes =
        std::fs::read(find(&common::evidence(&outcome), "fit.json.run.json").unwrap()).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/records-v1/workflow-failure.json");
    if std::env::var_os("BASECRAFT_CAPTURE_WORKFLOW_RECORDS").is_some() {
        std::fs::write(fixture, bytes).unwrap();
    } else {
        assert_eq!(bytes, std::fs::read(fixture).unwrap());
    }
    assert!(!root.path().join("single.spart").exists());
    drop(session);
    let result = records::replay(root.path()).unwrap();
    assert!(result.ok(), "{:?}", result.problems);
}

#[test]
fn asap_sampling_restores_roster_and_replays() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.fa");
    // a and b are identical; {a, b, c} and {d, e} are well separated.
    std::fs::write(
        &source,
        ">a\nACGTACGTACGTACGTACGT\n>b\nACGTACGTACGTACGTACGT\n>c\nACGTACGTACGTACGTACGA\n>d\nTGCATGCATGCATGCAACGT\n>e\nTGCATGCATGCATGCAACGA\n",
    )
    .unwrap();
    let registry = common::bundled();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    for unique in [false, true] {
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":"basecraft.host/asap_alignment@1",
                "inputs":{"alignment":common::file("input.fa")},
                "parameters":{"unique_haplotypes":unique,"model":"p","replicates":1000},
                "target":common::document(s, doc)[0]})
        });
        common::assert_completed(&outcome);
        let result = common::read_json(&common::artifact(&outcome, "fit.json"));
        let workspace = &result["workspace"];
        assert_eq!(workspace["specimens"].as_array().unwrap().len(), 5);
        let hypotheses = workspace["hypotheses"].as_array().unwrap();
        assert_ne!(hypotheses.len(), 0);
        for hypothesis in hypotheses {
            let mut members: Vec<String> = Vec::new();
            for group in hypothesis["groups"].as_array().unwrap() {
                let group: Vec<String> = serde_json::from_value(group["members"].clone()).unwrap();
                // A fitted representative carries its identical copies.
                if unique {
                    assert_eq!(group.contains(&"a".into()), group.contains(&"b".into()));
                }
                members.extend(group);
            }
            members.sort();
            assert_eq!(members, ["a", "b", "c", "d", "e"]);
        }
        let hypothesis = &hypotheses[0];
        assert_eq!(hypothesis["provenance"]["method"], "ASAP");
        assert!(hypothesis["provenance"]["analysis_tree"].is_null());
        let params = &hypothesis["provenance"]["parameters"];
        assert_eq!(params["deduplicate"], unique);
        assert_eq!(params["retained_count"], if unique { 4 } else { 5 });
        assert_eq!(params["replicates"], 1000);
        assert_eq!(params["stream"], "parallel");
        let snapshot = params["alignment_snapshot"].as_str().unwrap();
        assert_eq!(
            basecraft_io::hash_file(Path::new(snapshot)).unwrap(),
            params["alignment_input_hash"]
        );
        // The workspace links the full alignment, never the representatives.
        let linked = workspace["exchange_fields"]["basecraft_alignment_snapshot"]
            .as_str()
            .unwrap();
        assert_eq!(
            std::fs::read(linked).unwrap(),
            std::fs::read(&source).unwrap()
        );
        if unique {
            assert_eq!(
                hypothesis["provenance"]["haplotypes"]["a"],
                json!(["a", "b"])
            );
        } else {
            assert_eq!(hypothesis["provenance"]["haplotypes"], json!({}));
        }
    }
    // The alignment the runs analysed records both in its history.
    assert_eq!(
        finished(&session, doc)
            .iter()
            .filter(|(_, status)| *status == JobStatus::Completed)
            .count(),
        2
    );
    drop(session);
    let report = records::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn tree_methods_update_one_canonical_spart_and_export_without_sources() {
    isolated_profile::isolate!();
    require_engine();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(
        &source,
        include_str!("../../../testdata/fixtures/delimitation/gmyc-small.nwk"),
    )
    .unwrap();
    let registry = common::bundled();
    let mut session = Session::recoverable(&root).unwrap();
    let input = session.open(&source).unwrap();
    let canonical = root.join("species.spart");
    let setup = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::document(s, input)},
            "parameters":{},"destinations":{"workspace":"species.spart"}})
    });
    common::assert_completed(&setup);
    assert_eq!(common::visible(&root), ["source.nwk", "species.spart"]);
    let doc = session.open(&canonical).unwrap();
    let mut count = 0;
    for method in [
        "gmyc_single",
        "gmyc_multiple",
        "gmyc_exhaustive",
        "manual_cut",
        "validate_time_tree",
        "relative_time",
        "inspect_tree",
    ] {
        let operation = format!("basecraft.delimitation/{method}@1");
        let parameters = common::declared_only(
            &registry,
            &operation,
            &json!({"rooted":true,"threshold":0.25,"candidate_budget":100}),
        );
        // The workspace is the run's target: it records the run and receives
        // its hypotheses, as in the GUI.
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":operation,"inputs":{"tree":common::file("source.nwk")},
                "parameters":parameters,"target":common::document(s, doc)[0]})
        });
        common::assert_completed(&outcome);
        assert_eq!(
            finished(&session, doc)
                .iter()
                .filter(|(id, status)| id == &outcome.run_id && *status == JobStatus::Completed)
                .count(),
            1,
            "each applied result must record one successful terminal entry"
        );
        let value = common::read_json(&common::artifact(&outcome, "result.json"));
        if let Some(hypotheses) = value["workspace"]["hypotheses"].as_array() {
            count += hypotheses.len();
            if method == "manual_cut" {
                assert_eq!(value["metrics"]["groups"], json!([["a", "b"], ["c", "d"]]));
                assert_eq!(
                    hypotheses[0]["scores"].as_array().unwrap(),
                    &Vec::<Value>::new()
                );
            }
            session.save(doc, None).unwrap();
        }
        assert_eq!(
            basecraft_ops::files::load_delimitation(&canonical)
                .unwrap()
                .hypotheses
                .len(),
            count,
            "{method}"
        );
        assert_eq!(
            common::visible(&root),
            ["source.nwk", "species.spart"],
            "{method}"
        );
    }
    // A second workspace never replaces the canonical one.
    let before = std::fs::read(&canonical).unwrap();
    let refused = common::refused_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::document(s, input)},
            "parameters":{},"destinations":{"workspace":"species.spart"}})
    });
    assert!(refused.contains("output_conflict"), "{refused}");
    assert_eq!(before, std::fs::read(&canonical).unwrap());
    let issues = basecraft_ops::records::verify(&canonical).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let saved_folder = parent.path().join("saved-copy");
    std::fs::create_dir(&saved_folder).unwrap();
    let copied = saved_folder.join("species.spart");
    session.save(doc, Some(&copied)).unwrap();
    assert_eq!(
        basecraft_ops::records::verify(&copied).unwrap(),
        Vec::<String>::new()
    );
    let package = parent.path().join("package");
    basecraft_ops::records::package(&copied, &package).unwrap();
    let expected = session
        .delimitation(doc)
        .unwrap()
        .hypotheses
        .iter()
        .map(|h| h.groups.clone())
        .collect::<Vec<_>>();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    std::fs::rename(&saved_folder, parent.path().join("saved-unavailable")).unwrap();
    let relocated_package = parent.path().join("relocated-package");
    std::fs::rename(&package, &relocated_package).unwrap();
    let exported = relocated_package.join("species.spart");
    assert_eq!(
        basecraft_ops::records::verify(&exported).unwrap(),
        Vec::<String>::new()
    );
    let work = basecraft_ops::files::load_delimitation(&exported).unwrap();
    assert_eq!(
        work.hypotheses
            .iter()
            .map(|h| h.groups.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let display = basecraft_ops::delimitation::checked_display_tree(&work)
        .unwrap()
        .unwrap();
    assert!(display.is_file(), "{}", display.display());
    for h in &work.hypotheses {
        let report = &h.provenance.parameters["analysis_report"];
        assert_eq!(
            basecraft_io::hash_file(Path::new(report["path"].as_str().unwrap())).unwrap(),
            report["hash"]
        );
    }
}

#[test]
fn dirty_tree_submission_retains_generated_bytes_without_stale_original_claim() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    session
        .apply(
            doc,
            basecraft_protocol::Op::TreeRenameTip {
                from: "a".into(),
                to: "unsaved".into(),
            },
        )
        .unwrap();
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::document(s, doc)},
            "parameters":{},"destinations":{"workspace":"species.spart"}})
    });
    common::assert_completed(&outcome);
    let run = common::evidence(&outcome);
    let input = input_evidence::read_input(&run).unwrap();
    assert_eq!(
        input.state,
        basecraft_protocol::input_records::EvidenceState::Generated
    );
    assert!(input.source.is_none());
    let generated = run.join("submitted.fasta");
    assert!(
        std::fs::read_to_string(&generated)
            .unwrap()
            .contains("unsaved")
    );
    assert_eq!(
        input.submitted_hash,
        basecraft_io::hash_file(&generated).unwrap()
    );
    let canonical = root.path().join("species.spart");
    let work = basecraft_ops::files::load_delimitation(&canonical).unwrap();
    assert!(work.specimens.iter().any(|s| s.id == "unsaved"));
    assert!(!work.specimens.iter().any(|s| s.id == "a"));
    // Specimens cite the retained submitted bytes, never a temporary copy.
    assert!(
        work.specimens
            .iter()
            .all(|s| s.sources.contains_key(generated.to_str().unwrap())),
        "{:?}",
        work.specimens
    );
    // The retained bytes are a valid input for a later fit in that workspace.
    let workspace = session.open(&canonical).unwrap();
    let relative = generated.strip_prefix(root.path()).unwrap();
    let fitted = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.delimitation/gmyc_single@1",
            "inputs":{"tree":common::file(relative.to_str().unwrap())},
            "parameters":{"rooted":true},"target":common::document(s, workspace)[0]})
    });
    common::assert_completed(&fitted);
    session.save(workspace, None).unwrap();
    // GMYC publishes each distinct partition in its support set, best first.
    let hypotheses = basecraft_ops::files::load_delimitation(&canonical)
        .unwrap()
        .hypotheses;
    assert_ne!(hypotheses.len(), 0);
    assert!(
        hypotheses
            .iter()
            .all(|h| h.name.starts_with("GMYC species hypothesis "))
    );
    assert!(std::fs::read_to_string(source).unwrap().contains("a:"));
}

#[test]
fn ptp_and_mptp_keep_upstream_directories_hidden_and_preserve_imported_groups() {
    isolated_profile::isolate!();
    // The plugin finds mPTP through the environment, so the cases run in a
    // child process that names this storage fixture as mPTP.
    if std::env::var_os("BASECRAFT_MPTP_FIXTURE_CHILD").is_some() {
        ptp_cases();
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let upstream = fixture.path().join("mptp-fixture");
    std::fs::write(&upstream,r"#!/usr/bin/env python3
import sys
from pathlib import Path
if '--version' in sys.argv:
    print('storage contract fixture, not mPTP inference')
else:
    output = sys.argv[sys.argv.index('--output_file')+1]
    rate = 'single' if '--single' in sys.argv else 'multi'
    Path(output+'.txt').write_text('Best score for '+rate+' coalescent rate: -1\nNull-model score: -2\nNumber of delimited species: 2\nLRT: passed\nSpecies 1:\na\nb\nSpecies 2:\nc\nd\n')
").unwrap();
    std::fs::set_permissions(&upstream, std::fs::Permissions::from_mode(0o700)).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "ptp_and_mptp_keep_upstream_directories_hidden_and_preserve_imported_groups",
            "--nocapture",
        ])
        .env("BASECRAFT_MPTP_FIXTURE_CHILD", "1")
        .env("BASECRAFT_MPTP", &upstream)
        .status()
        .unwrap();
    assert!(status.success());
}

fn ptp_cases() {
    let registry = common::bundled();
    for method in ["ptp", "mptp"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.nwk");
        std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
        let mut session = Session::new(root.path());
        let doc = session.open(&source).unwrap();
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":format!("basecraft.delimitation/{method}@1"),
                "inputs":{"tree":common::document(s, doc)},
                "parameters":{"rooted":true},"destinations":{"workspace":"species.spart"}})
        });
        common::assert_completed(&outcome);
        let work =
            basecraft_ops::files::load_delimitation(&root.path().join("species.spart")).unwrap();
        assert_eq!(
            work.hypotheses[0]
                .groups
                .iter()
                .map(|g| g.members.clone())
                .collect::<Vec<_>>(),
            vec![
                ["a".into(), "b".into()].into(),
                ["c".into(), "d".into()].into()
            ]
        );
        assert_eq!(work.hypotheses[0].provenance.method, method);
        assert_eq!(work.hypotheses[0].scores[0].value, -1.0);
        assert_eq!(
            common::visible(root.path()),
            ["source.nwk", "species.spart"]
        );
        // The upstream program's log is retained with the run, never visible.
        assert!(
            outcome
                .artifacts
                .iter()
                .any(|a| a.path.ends_with("stdout.log")
                    && Path::new(&a.path).starts_with(root.path().join(".basecraft"))),
            "{outcome:#?}"
        );
        assert_eq!(
            basecraft_ops::records::verify(&root.path().join("species.spart")).unwrap(),
            Vec::<String>::new()
        );
    }
}

#[test]
fn concurrent_species_publication_never_replaces_another_run() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let canonical = root.path().join("species.spart");
    let request = json!({"operation":"basecraft.delimitation/workspace_tree@1",
        "inputs":{"tree":common::file("source.nwk")},
        "parameters":{},"destinations":{"workspace":"species.spart"}});
    // Both runs are accepted before either publishes; their runs are distinct.
    let mut first = JobManager::default();
    let mut second = JobManager::default();
    let policy = || basecraft_plugin_host::policy::SessionPolicy::new(root.path(), true).unwrap();
    let one = first
        .submit(
            &registry,
            serde_json::from_value(request.clone()).unwrap(),
            policy(),
            "test",
        )
        .unwrap();
    let two = second
        .submit(
            &registry,
            serde_json::from_value(request).unwrap(),
            policy(),
            "test",
        )
        .unwrap();
    assert_ne!(one.evidence, two.evidence);
    let one = common::wait(&first, &one.run_id);
    let two = common::wait(&second, &two.run_id);
    let completed = [&one, &two]
        .into_iter()
        .filter(|outcome| outcome.state == RuntimeState::Completed)
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1, "{one:#?}\n{two:#?}");
    let winner = completed[0];
    assert_eq!(
        common::visible(root.path()),
        ["source.nwk", "species.spart"]
    );
    assert_eq!(
        basecraft_ops::analysis_store::ArtifactRun::for_result(&canonical)
            .unwrap()
            .unwrap(),
        common::evidence(winner)
    );
    assert_eq!(
        basecraft_ops::files::load_delimitation(&canonical)
            .unwrap()
            .specimens
            .len(),
        4
    );
    for outcome in [&one, &two] {
        assert!(common::evidence(outcome).join("execution.json").is_file());
    }
}

#[test]
fn optional_display_tree_uses_hidden_reference_first_companions() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(
        &source,
        ">a description\nAAAAA\nAAAAA\n>b\nAAAAAACCAA\n>c\nAAAAGGCCAA\n>d\nAATTGGCCAA\n",
    )
    .unwrap();
    let registry = common::bundled();
    let mut session = Session::recoverable(root.path()).unwrap();
    let input = session.open(&source).unwrap();
    let canonical = root.path().join("species.spart");
    let setup = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.delimitation/workspace_alignment@1",
            "inputs":{"alignment":common::document(s, input)},
            "parameters":{},"destinations":{"workspace":"species.spart"}})
    });
    common::assert_completed(&setup);
    let workspace = session.open(&canonical).unwrap();
    // A display tree reads the referenced alignment and is exported as a file,
    // recorded in the workspace's history, as the GUI does.
    let display = root.path().join("display.nwk");
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.distance/tree@1",
            "inputs":{"alignment":common::file("source.afa")},
            "parameters":{"method":"upgma","model":"p","min_overlap":1},
            "destinations":{"artifacts/tree.nwk":"display.nwk"},
            "target":common::document(s, workspace)[0]})
    });
    common::assert_completed(&outcome);
    assert_eq!(
        common::visible(root.path()),
        ["display.nwk", "source.afa", "species.spart"]
    );
    let run = common::evidence(&outcome);
    assert_eq!(
        input_evidence::read_input(&run).unwrap().state,
        basecraft_protocol::input_records::EvidenceState::Referenced
    );
    assert!(!run.join("submitted.fasta").exists());
    let tree = basecraft_ops::files::load_tree(&display).unwrap();
    let link = basecraft_ops::records::AlignmentLink::read(&display).unwrap();
    // The link resolves to exactly the alignment the run read: the referenced
    // source in the canonical FASTA the descriptor's view submits.
    assert_eq!(link.source, source);
    let snapshot = link.checked_snapshot(&display, &tree.doc).unwrap();
    let basecraft_ops::files::Loaded::Alignment(alignment) =
        basecraft_ops::files::load(&source).unwrap()
    else {
        panic!("source is not an alignment")
    };
    let mut canonical = Vec::new();
    basecraft_ops::files::write_alignment(&alignment.doc, &mut canonical).unwrap();
    assert_eq!(
        basecraft_io::hash_file(&snapshot).unwrap(),
        basecraft_io::hash_bytes(&canonical)
    );
    assert_eq!(
        basecraft_io::hash_file(&snapshot).unwrap(),
        link.snapshot_hash
    );
    assert_eq!(
        basecraft_ops::records::verify(&display).unwrap(),
        Vec::<String>::new()
    );
    assert!(
        common::tool_runs(&session, workspace)
            .iter()
            .any(|run| run["run_id"] == outcome.run_id.as_str())
    );
}

#[test]
fn alignment_workflows_use_frozen_unsaved_inputs() {
    isolated_profile::isolate!();
    require_engine();
    for (operation, dirty) in [
        ("asap_alignment", true),
        ("gmyc_alignment_single", true),
        ("asap_alignment", false),
        ("gmyc_alignment_single", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.afa");
        let original = ">a first specimen\nACGTACGTACGTACGTACGT\n>b second specimen\nACGTACGTACGTACGTACGA\n>c\nTGCATGCATGCATGCAACGT\n>d\nTGCATGCATGCATGCAACGA\n";
        std::fs::write(&source, original).unwrap();
        let registry = common::bundled();
        let mut session = Session::new(root.path());
        let doc = session.open(&source).unwrap();
        if dirty {
            session
                .apply(
                    doc,
                    basecraft_protocol::Op::AlnReplace {
                        row: 0,
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
        }
        let mut manager = JobManager::default();
        let accepted = common::submit_in(&mut manager, &registry, &mut session, |s| {
            json!({
                "operation":format!("basecraft.host/{operation}@1"),
                "inputs":{"alignment":common::document(s, doc)},
                "parameters":{"model":"p","unique_haplotypes":false}
            })
        })
        .unwrap();
        // The accepted generated input is independent of its original path.
        std::fs::write(&source, ">unrelated\nAAAA\n").unwrap();
        let outcome = common::wait_in(&manager, &mut session, &accepted.run_id);
        common::assert_completed(&outcome);
        let input = input_evidence::read_input(&common::evidence(&outcome)).unwrap();
        assert_eq!(
            input.state,
            if dirty {
                basecraft_protocol::input_records::EvidenceState::Generated
            } else {
                basecraft_protocol::input_records::EvidenceState::Referenced
            }
        );
        let submitted = common::evidence(&outcome).join("submitted.fasta");
        if dirty {
            assert_eq!(
                input.submitted_hash,
                basecraft_io::hash_file(&submitted).unwrap()
            );
            assert!(
                std::fs::read_to_string(submitted)
                    .unwrap()
                    .contains("TCGTACGT")
            );
        }
        let result = common::read_json(&common::artifact(&outcome, "fit.json"));
        assert_eq!(
            result["workspace"]["specimens"].as_array().unwrap().len(),
            4
        );
    }
}

#[test]
fn parent_cancellation_interrupts_registered_fit_and_retains_every_stage_receipt() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.fa");
    std::fs::write(&source, ">a\nAAAAAAAAAAAAAAAAAAAA\n>b\nAAAAAAAAAAAAAAAAAAAT\n>c\nAAAAACAAAAAAAAAAAAAT\n>d\nAAAAACAAAAAGAAAAAAAT\n>e\nAAAAACAAAAAGAAAAAATT\n").unwrap();
    // Reuse the public adapter and real preparation engine. Only ASAP execution
    // is held: an accepted stage receipt alone cannot prove the fit is running,
    // and even a large simulation ceiling permits ASAP's early stopping.
    let bundled = common::fixture_package(
        root.path(),
        "delimitation",
        "asap",
        r"
from pathlib import Path
import delimitation_plugin as adapter
adapter.executable = lambda params: str(Path(__file__).with_name('engine.py'))
adapter.plugin.contract()
adapter.plugin.run()
",
    );
    let package = bundled.join("delimitation");
    let manifest = std::fs::read_to_string(common::plugins().join("delimitation/plugin.toml"))
        .unwrap()
        .replace("python:delimitation_plugin.py", "python:fixture.py");
    std::fs::write(package.join("plugin.toml"), manifest).unwrap();
    let host = bundled.join("host-workflows");
    std::fs::create_dir(&host).unwrap();
    for name in ["plugin.toml", "operations.json", "host_workflows.py"] {
        std::fs::copy(
            common::plugins().join("host-workflows").join(name),
            host.join(name),
        )
        .unwrap();
    }
    for name in ["operations.json", "delimitation_plugin.py"] {
        std::fs::copy(
            common::plugins().join("delimitation").join(name),
            package.join(name),
        )
        .unwrap();
    }
    let engine = package.join("engine.py");
    let real_engine = std::env::var("BASECRAFT_DELIMITATION").unwrap();
    std::fs::write(
        &engine,
        format!(
            r"#!/usr/bin/env python3
import json, os, sys, time
from pathlib import Path
if len(sys.argv) == 3 and sys.argv[1] == '--request':
    request = json.loads(Path(sys.argv[2]).read_text())
    if request['method'] == 'asap':
        gate = Path(__file__).with_name('release-fit')
        ready = Path(__file__).with_name('fit-ready')
        ready.with_suffix('.tmp').write_text(json.dumps(request))
        ready.with_suffix('.tmp').replace(ready)
        deadline = time.monotonic() + 60
        while not gate.exists():
            if time.monotonic() >= deadline:
                raise RuntimeError('test did not cancel the held fit')
            time.sleep(.01)
os.execv({real_engine}, [{real_engine}, *sys.argv[1:]])
",
            real_engine = serde_json::to_string(&real_engine).unwrap()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    let registry = common::registry(&bundled);
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let generation = session.generation(doc).unwrap();
    let mut manager = JobManager::default();
    let accepted = common::submit_in(&mut manager, &registry, &mut session, |s| json!({
        "operation":"basecraft.host/asap_alignment@1", "inputs":{"alignment":common::document(s,doc)},
        "parameters":{"model":"p","replicates":1000},
        "destinations":{"workspace":"cancelled.spart"}
    })).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if package.join("fit-ready").is_file() {
            break;
        }
        let outcome = manager.get(&accepted.run_id).unwrap();
        if outcome.state.terminal() {
            let retained = root.keep();
            panic!(
                "workflow stopped before fitting: {outcome:#?}; evidence: {}",
                retained.display()
            );
        }
        assert!(std::time::Instant::now() < deadline, "fit did not start");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let held_request = common::read_json(&package.join("fit-ready"));
    let child_cancel = Path::new(held_request["cancel_file"].as_str().unwrap());
    assert!(!child_cancel.exists());
    assert!(!package.join("release-fit").exists());
    manager.cancel(&accepted.run_id).unwrap();
    let cancelled = common::wait_in(&manager, &mut session, &accepted.run_id);
    assert_eq!(cancelled.state, RuntimeState::Cancelled);
    assert_eq!(session.generation(doc).unwrap(), generation);
    assert_eq!(
        finished(&session, doc),
        [(accepted.run_id, JobStatus::Cancelled)]
    );
    let stages: Vec<basecraft_protocol::operation::OperationOutcome> = serde_json::from_value(
        common::read_json(&common::evidence(&cancelled).join("stages.json")),
    )
    .unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].state, RuntimeState::Completed);
    assert_eq!(stages[1].state, RuntimeState::Cancelled);
    // This path belongs to the child: parent-only cancellation is insufficient.
    assert!(child_cancel.is_file());
    assert!(!package.join("release-fit").exists());
    for stage in stages {
        let receipt = common::read_json(&common::evidence(&stage).join("runtime.json"));
        assert_eq!(receipt["state"], serde_json::to_value(stage.state).unwrap());
    }
    // The workflow writes working output before the runner publishes artifacts.
    // Neither boundary may expose a successful fit from this held child.
    for path in ["workflow/fit.json", "artifacts/fit.json"] {
        assert!(!common::evidence(&cancelled).join(path).exists());
    }
    assert_eq!(cancelled.artifacts, Vec::new());
    assert!(!root.path().join("cancelled.spart").exists());
}

#[test]
fn workflow_repairs_publication_and_applies_all_hypotheses_in_one_undo_group() {
    isolated_profile::isolate!();
    require_engine();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("input.fa"), ">a\nAAAAAAAAAAAAAAAAAAAA\n>b\nAAAAAAAAAAAAAAAAAAAT\n>c\nAAAAACAAAAAAAAAAAAAT\n>d\nAAAAACAAAAAGAAAAAAAT\n>e\nAAAAACAAAAAGAAAAAATT\n").unwrap();
    let registry = common::bundled();
    let setup = common::run(
        &registry,
        root.path(),
        json!({"operation":"basecraft.delimitation/workspace_alignment@1",
        "inputs":{"alignment":common::file("input.fa")}, "parameters":{},"destinations":{"workspace":"species.spart"}}),
    );
    common::assert_completed(&setup);
    let mut session = Session::new(root.path());
    let doc = session.open(&root.path().join("species.spart")).unwrap();
    let before = session.delimitation(doc).unwrap().hypotheses.clone();
    let mut manager = JobManager::default();
    let accepted = common::submit_in(&mut manager,&registry,&mut session,|s| json!({
        "operation":"basecraft.host/asap_alignment@1","inputs":{"alignment":common::file("input.fa")},
        "parameters":{"model":"p","unique_haplotypes":true,"replicates":1000},
        "target":common::document(s,doc)[0],"destinations":{"workspace":"fit.spart"}
    })).unwrap();
    let export = root.path().join("fit.spart");
    std::fs::write(&export, "foreign").unwrap();
    let partial = common::wait_in(&manager, &mut session, &accepted.run_id);
    assert_eq!(partial.state, RuntimeState::Partial, "{partial:#?}");
    assert_eq!(session.delimitation(doc).unwrap().hypotheses, before);
    assert_eq!(std::fs::read_to_string(&export).unwrap(), "foreign");
    let receipt = common::evidence(&partial).join("stages.json");
    let stages = std::fs::read(&receipt).unwrap();
    std::fs::remove_file(&export).unwrap();
    manager.repair(&accepted.run_id).unwrap();
    let complete = common::wait_in(&manager, &mut session, &accepted.run_id);
    common::assert_completed(&complete);
    assert_eq!(
        std::fs::read(receipt).unwrap(),
        stages,
        "repair must never execute stages again"
    );
    let applied = session.delimitation(doc).unwrap().hypotheses.clone();
    assert!(applied.len() > before.len());
    assert_eq!(
        finished(&session, doc)
            .iter()
            .filter(|(id, status)| id == &complete.run_id && *status == JobStatus::Completed)
            .count(),
        1
    );
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc).unwrap().hypotheses, before);
    session.redo(doc).unwrap();
    assert_eq!(session.delimitation(doc).unwrap().hypotheses, applied);
    assert_eq!(
        basecraft_ops::records::verify(&export).unwrap(),
        Vec::<String>::new()
    );
    drop(session);
    assert!(records::replay(root.path()).unwrap().ok());
}

#[test]
fn workflow_save_decision_covers_nested_stages_and_original_ancestry() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    isolated_profile::isolate!();
    require_engine();
    for policy in [
        SavePolicy::PreserveCopy,
        SavePolicy::BreakEvidence,
        SavePolicy::RemoveAnalysis,
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.fa");
        let original = ">a\nAAAAAAAAAAAAAAAAAAAA\n>b\nAAAAAAAAAAAAAAAAAAAT\n>c\nAAAAACAAAAAAAAAAAAAT\n>d\nAAAAACAAAAAGAAAAAAAT\n>e\nAAAAACAAAAAGAAAAAATT\n";
        std::fs::write(&source, original).unwrap();
        let outcome = common::run(
            &common::bundled(),
            root.path(),
            json!({
                "operation":"basecraft.host/asap_alignment@1","inputs":{"alignment":common::file("input.fa")},
                "parameters":{"model":"p","unique_haplotypes":true,"replicates":1000},
                "destinations":{"workspace":"species.spart"}
            }),
        );
        common::assert_completed(&outcome);
        let stages: Vec<basecraft_protocol::operation::OperationOutcome> = serde_json::from_value(
            common::read_json(&common::evidence(&outcome).join("stages.json")),
        )
        .unwrap();
        let conflict = conflicts(&source).unwrap();
        assert_eq!(
            conflict.analyses.len(),
            1,
            "one logical analysis including all stages"
        );
        let backing = &conflict.analyses[0].backing_analyses;
        assert!(
            backing.len() > stages.len() + 1,
            "dedicated input ledgers remain in the decision"
        );
        for stage in &stages {
            assert!(
                backing.iter().any(|a| a.run == common::evidence(stage)),
                "stage must depend on the original ancestry"
            );
        }
        resolve(&conflict, policy).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        if policy == SavePolicy::PreserveCopy {
            assert_eq!(
                backing
                    .iter()
                    .filter(|a| a.run.join("preserved-input").exists())
                    .count(),
                1
            );
            std::fs::remove_file(&source).unwrap();
            assert_eq!(
                basecraft_ops::records::verify(&root.path().join("species.spart")).unwrap(),
                Vec::<String>::new()
            );
            for stage in stages {
                assert_eq!(
                    basecraft_ops::records::verify(Path::new(&stage.artifacts[0].path)).unwrap(),
                    Vec::<String>::new()
                );
            }
        } else {
            for affected in backing {
                if policy == SavePolicy::BreakEvidence {
                    assert_eq!(
                        input_evidence::read_input(&affected.run).unwrap().state,
                        basecraft_protocol::input_records::EvidenceState::Broken
                    );
                } else {
                    assert!(!affected.run.exists());
                }
            }
            assert!(
                !matches!(basecraft_ops::records::verify(&root.path().join("species.spart")),Ok(issues) if issues.is_empty())
            );
        }
    }
}
