//! Actual public socket/SDK/native-engine workflows through registered
//! operations; no mock numerical results.
mod common;

use basecraft_ops::{Session, input_evidence, replay};
use basecraft_plugin_host::jobs::JobManager;
use basecraft_protocol::{delimitation::JobStatus, operation::RuntimeState};
use serde_json::{Value, json};
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
        .into_iter()
        .filter_map(|event| match event.op {
            basecraft_protocol::Op::JobFinished { report } => Some((report.id, report.status)),
            _ => None,
        })
        .collect()
}

#[test]
fn snapshots_and_terminal_outcomes_survive_success_and_failure() {
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
        assert_eq!(input.state, "referenced");
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
    let report = replay::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn exact_identity_precedes_public_tree_inference_and_restores_specimens() {
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
    assert!(!root.path().join("single.spart").exists());
    drop(session);
    let result = replay::replay(root.path()).unwrap();
    assert!(result.ok(), "{:?}", result.problems);
}

#[test]
fn asap_sampling_restores_roster_and_replays() {
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
    let report = replay::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn tree_methods_update_one_canonical_spart_and_export_without_sources() {
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
    let issues = basecraft_ops::provenance::verify(&canonical).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let saved_folder = parent.path().join("saved-copy");
    std::fs::create_dir(&saved_folder).unwrap();
    let copied = saved_folder.join("species.spart");
    session.save(doc, Some(&copied)).unwrap();
    assert_eq!(
        basecraft_ops::provenance::verify(&copied).unwrap(),
        Vec::<String>::new()
    );
    let package = parent.path().join("package");
    basecraft_ops::provenance::package(&copied, &package).unwrap();
    let expected = session
        .delimitation(doc)
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
        basecraft_ops::provenance::verify(&exported).unwrap(),
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
    assert_eq!(input.state, "generated");
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
    use std::os::unix::fs::PermissionsExt;
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
            basecraft_ops::provenance::verify(&root.path().join("species.spart")).unwrap(),
            Vec::<String>::new()
        );
    }
}

#[test]
fn concurrent_species_publication_never_replaces_another_run() {
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
        "referenced"
    );
    assert!(!run.join("submitted.fasta").exists());
    let tree = basecraft_ops::files::load_tree(&display).unwrap();
    let link = basecraft_ops::links::AlignmentLink::read(&display).unwrap();
    let snapshot = link.checked_snapshot(&display, &tree.doc).unwrap();
    assert_eq!(
        basecraft_io::hash_file(&snapshot).unwrap(),
        basecraft_io::hash_file(&source).unwrap()
    );
    assert_eq!(
        basecraft_ops::provenance::verify(&display).unwrap(),
        Vec::<String>::new()
    );
    assert!(
        common::tool_runs(&session, workspace)
            .iter()
            .any(|run| run["run_id"] == outcome.run_id.as_str())
    );
}
