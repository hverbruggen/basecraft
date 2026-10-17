//! Actual public socket/SDK/native-engine workflow; no mock numerical results.
use basecraft_ops::{Session, replay};
use basecraft_plugin_host::FileJob;
use serde_json::{Value, json};
use std::path::Path;

#[test]
fn snapshots_and_terminal_outcomes_survive_success_failure_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tree.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    let native = std::env::var_os("BASECRAFT_DELIMITATION").map_or_else(
        || {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("basecraft-delimitation")
        },
        std::path::PathBuf::from,
    );
    assert!(
        native.is_file(),
        "Build basecraft-delimitation before this integration test"
    );
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    for (name, rooted, cancel) in [
        ("success", true, false),
        ("failed", false, false),
        ("cancelled", true, true),
    ] {
        let output = root.path().join(format!("{name}.json"));
        let mut job =
            FileJob::from_tree(&session, doc, (&plugin, "gmyc_single"), output.clone()).unwrap();
        job.params.extend([
            ("executable".into(), json!(native)),
            ("rooted".into(), json!(rooted)),
            ("generation".into(), json!(session.generation(doc))),
        ]);
        if cancel {
            let path = root.path().join("cancel");
            std::fs::write(&path, "").unwrap();
            job.params.insert("cancel_file".into(), json!(path));
        }
        let started = std::time::Instant::now();
        let outcome = job.run();
        job.record_terminal(&mut session, doc, &outcome, started.elapsed().as_secs_f64())
            .unwrap();
        let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
        let run_directory = paths
            .state
            .join("jobs")
            .join(job.params["job_id"].as_str().unwrap());
        assert!(!run_directory.join("input.snapshot").exists());
        let evidence = job.species_input_evidence().unwrap();
        assert_eq!(evidence["state"], "referenced");
        assert_eq!(
            basecraft_io::hash_file(Path::new(evidence["path"].as_str().unwrap())).unwrap(),
            evidence["source_hash"]
        );
        assert!(!output.exists());
        assert!(
            job.output
                .starts_with(root.path().join(".basecraft/analyses-v1/runs"))
        );
        let run: Value =
            serde_json::from_slice(&std::fs::read(run_directory.join("manifest.json")).unwrap())
                .unwrap();
        let report = json!({"job": run});
        assert_eq!(
            report["job"]["status"],
            if rooted && !cancel {
                "completed"
            } else if cancel {
                "cancelled"
            } else {
                "failed"
            }
        );
        assert_eq!(report["job"]["id"], job.params["job_id"]);
        if name == "success" {
            let result = outcome.unwrap();
            job.record(&mut session, doc, &result).unwrap();
            assert!(result.outputs.iter().any(|p| {
                Path::new(p)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("spart"))
            }));
        } else {
            assert!(outcome.is_err());
            assert!(!Path::new(&format!("{}.spart", output.display())).exists());
        }
    }
    drop(session);
    let report = replay::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn exact_identity_precedes_public_tree_inference_and_restores_specimens() {
    use basecraft_plugin_host::{delimitation_pipeline::AlignmentJob, file_tool::AlignmentInput};
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
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    let native = std::env::var_os("BASECRAFT_DELIMITATION").map_or_else(
        || {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("basecraft-delimitation")
        },
        std::path::PathBuf::from,
    );
    assert!(
        native.is_file(),
        "Build basecraft-delimitation before this integration test"
    );
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    for (unique, name) in [(true, "unique"), (false, "all")] {
        let output = root.path().join(format!("{name}.json"));
        let mut preparation = FileJob::from_alignment(
            &session,
            doc,
            (&plugin, "haplotypes"),
            AlignmentInput::Aligned,
            root.path().join(format!("{name}.prepare.json")),
        )
        .unwrap();
        preparation.params.extend([
            ("executable".into(), json!(native)),
            ("model".into(), json!("p")),
        ]);
        let pipeline = AlignmentJob {
            preparation,
            unique_haplotypes: unique,
            fit_method: "gmyc_single".into(),
            output: output.clone(),
        }
        .run();
        let output = pipeline.output.clone();
        let outcome = pipeline.record(&mut session, doc);
        if unique {
            let result = outcome.unwrap();
            assert_eq!(
                result.outputs[0],
                root.path().join("unique.spart").display().to_string()
            );
            let result: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
            assert_eq!(
                result["workspace"]["specimens"].as_array().unwrap().len(),
                5
            );
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
            assert!(outcome.is_err());
            assert!(!output.exists());
            let report: Value = serde_json::from_slice(
                &std::fs::read(format!("{}.run.json", output.display())).unwrap(),
            )
            .unwrap();
            assert_eq!(report["job"]["status"], "failed");
            assert!(
                report["job"]["diagnostics"]
                    .to_string()
                    .contains("zero-time")
            );
        }
    }
    let single = root.path().join("single.fasta");
    std::fs::write(&single, ">a\nACGT\n>b\nACGT\n").unwrap();
    let single_doc = session.open(&single).unwrap();
    let mut preparation = FileJob::from_alignment(
        &session,
        single_doc,
        (&plugin, "haplotypes"),
        AlignmentInput::Aligned,
        root.path().join("single.prepare.json"),
    )
    .unwrap();
    preparation
        .params
        .insert("executable".into(), json!(native));
    let output = root.path().join("single.json");
    let pipeline = AlignmentJob {
        preparation,
        unique_haplotypes: true,
        fit_method: "gmyc_single".into(),
        output,
    }
    .run();
    let output = pipeline.output.clone();
    assert!(pipeline.record(&mut session, single_doc).is_err());
    let report: Value =
        serde_json::from_slice(&std::fs::read(format!("{}.run.json", output.display())).unwrap())
            .unwrap();
    assert_eq!(report["job"]["status"], "unavailable");
    assert!(!output.exists());
    drop(session);
    let result = replay::replay(root.path()).unwrap();
    assert!(result.ok(), "{:?}", result.problems);
}

#[test]
fn asap_sampling_restores_roster_and_replays() {
    use basecraft_plugin_host::delimitation_pipeline::AlignmentJob;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.fa");
    // a and b are identical; {a, b, c} and {d, e} are well separated.
    std::fs::write(
        &source,
        ">a\nACGTACGTACGTACGTACGT\n>b\nACGTACGTACGTACGTACGT\n>c\nACGTACGTACGTACGTACGA\n>d\nTGCATGCATGCATGCAACGT\n>e\nTGCATGCATGCATGCAACGA\n",
    )
    .unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    let native = std::env::var_os("BASECRAFT_DELIMITATION").map_or_else(
        || {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("basecraft-delimitation")
        },
        std::path::PathBuf::from,
    );
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    for unique in [false, true] {
        let output = root.path().join(format!("asap-{unique}.json"));
        let mut preparation = FileJob::from_file(
            &session,
            doc,
            &source,
            (&plugin, "haplotypes"),
            root.path().join(format!("prep-{unique}.json")),
        )
        .unwrap();
        preparation.params.extend([
            ("executable".into(), json!(native)),
            ("model".into(), json!("p")),
            ("replicates".into(), json!(1000)),
        ]);
        let pipeline = AlignmentJob {
            preparation,
            unique_haplotypes: unique,
            fit_method: "asap".into(),
            output: output.clone(),
        }
        .run();
        let output = pipeline.output.clone();
        pipeline.record(&mut session, doc).unwrap();
        let result: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let workspace = &result["workspace"];
        assert_eq!(workspace["specimens"].as_array().unwrap().len(), 5);
        let hypotheses = workspace["hypotheses"].as_array().unwrap();
        assert_ne!(hypotheses.len(), 0);
        for hypothesis in hypotheses {
            let mut members: Vec<String> = Vec::new();
            for group in hypothesis["groups"].as_array().unwrap() {
                let group: Vec<String> = serde_json::from_value(group["members"].clone()).unwrap();
                // Identical sequences always share a subset.
                assert_eq!(group.contains(&"a".into()), group.contains(&"b".into()));
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
    drop(session);
    let report = replay::replay(root.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

fn visible_files(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".basecraft")
        .collect();
    names.sort();
    names
}

#[test]
fn tree_methods_update_one_canonical_spart_and_export_without_sources() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(
        &source,
        include_str!("../../../testdata/fixtures/delimitation/gmyc-small.nwk"),
    )
    .unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    let mut session = Session::recoverable(&root).unwrap();
    let input = session.open(&source).unwrap();
    let canonical = root.join("species.spart");
    let setup = FileJob::from_tree(
        &session,
        input,
        (&plugin, "workspace_tree"),
        canonical.clone(),
    )
    .unwrap();
    let outcome = setup.run();
    setup
        .record_terminal(&mut session, input, &outcome, 0.0)
        .unwrap();
    setup
        .record(&mut session, input, &outcome.unwrap())
        .unwrap();
    assert_eq!(visible_files(&root), ["source.nwk", "species.spart"]);
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
        let mut job = FileJob::from_file(
            &session,
            doc,
            &source,
            (&plugin, method),
            root.join(format!("{method}.json")),
        )
        .unwrap();
        job.params.extend([
            ("rooted".into(), json!(true)),
            ("threshold".into(), json!(0.25)),
            ("candidate_budget".into(), json!(100)),
        ]);
        let outcome = job.run();
        job.record_terminal(&mut session, doc, &outcome, 0.0)
            .unwrap();
        job.record(&mut session, doc, &outcome.unwrap()).unwrap();
        let value =
            basecraft_ops::delimitation::add_analysis_result(&mut session, doc, &job.output)
                .unwrap();
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
            count
        );
        assert_eq!(
            visible_files(&root),
            ["source.nwk", "species.spart"],
            "{method}"
        );
    }
    let duplicate = FileJob::from_tree(
        &session,
        input,
        (&plugin, "workspace_tree"),
        canonical.clone(),
    )
    .unwrap();
    let before = std::fs::read(&canonical).unwrap();
    assert!(duplicate.run().is_err());
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
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
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
    let job = FileJob::from_tree(
        &session,
        doc,
        (&plugin, "workspace_tree"),
        root.path().join("species.spart"),
    )
    .unwrap();
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    let evidence = job.species_input_evidence().unwrap();
    assert_eq!(evidence["state"], "generated");
    assert!(evidence["source_hash"].is_null());
    let generated = Path::new(evidence["path"].as_str().unwrap());
    assert!(
        std::fs::read_to_string(generated)
            .unwrap()
            .contains("unsaved")
    );
    assert_eq!(
        job.input_io().unwrap()["persistent_submitted_copy_bytes"],
        std::fs::metadata(generated).unwrap().len()
    );
    let work = basecraft_ops::files::load_delimitation(&root.path().join("species.spart")).unwrap();
    assert!(work.specimens.iter().any(|s| s.id == "unsaved"));
    assert!(!work.specimens.iter().any(|s| s.id == "a"));
    assert!(
        work.specimens
            .iter()
            .all(|s| s.sources.contains_key(generated.to_str().unwrap()))
    );
    let workspace = session.open(&root.path().join("species.spart")).unwrap();
    let mut reused = FileJob::from_file(
        &session,
        workspace,
        generated,
        (&plugin, "gmyc_single"),
        root.path().join("reuse.json"),
    )
    .unwrap();
    reused.params.insert("rooted".into(), json!(true));
    let result = reused.run().unwrap();
    reused.record(&mut session, workspace, &result).unwrap();
    basecraft_ops::delimitation::add_analysis_result(&mut session, workspace, &reused.output)
        .unwrap();
    session.save(workspace, None).unwrap();
    assert_eq!(
        basecraft_ops::files::load_delimitation(&root.path().join("species.spart"))
            .unwrap()
            .hypotheses
            .len(),
        1
    );

    assert!(std::fs::read_to_string(source).unwrap().contains("a:"));
}

#[test]
fn ptp_and_mptp_keep_upstream_directories_hidden_and_preserve_imported_groups() {
    use std::os::unix::fs::PermissionsExt;
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
    for method in ["ptp", "mptp"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.nwk");
        std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
        let mut session = Session::new(root.path());
        let doc = session.open(&source).unwrap();
        let mut job = FileJob::from_tree(
            &session,
            doc,
            (&plugin, method),
            root.path().join("species.spart"),
        )
        .unwrap();
        job.params.extend([
            ("mptp_executable".into(), json!(upstream)),
            ("rooted".into(), json!(true)),
        ]);
        let outcome = job.run();
        job.record_terminal(&mut session, doc, &outcome, 0.0)
            .unwrap();
        let result = outcome.unwrap();
        job.record(&mut session, doc, &result).unwrap();
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
        assert_eq!(visible_files(root.path()), ["source.nwk", "species.spart"]);
        assert!(result.outputs.iter().any(|p| p.ends_with("stdout.log")
            && Path::new(p).starts_with(root.path().join(".basecraft"))));
        assert_eq!(
            basecraft_ops::provenance::verify(&root.path().join("species.spart")).unwrap(),
            Vec::<String>::new()
        );
    }
}

#[test]
fn concurrent_species_publication_never_replaces_another_run() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    let canonical = root.path().join("species.spart");
    let first = FileJob::from_tree(
        &session,
        doc,
        (&plugin, "workspace_tree"),
        canonical.clone(),
    )
    .unwrap();
    let second = FileJob::from_tree(
        &session,
        doc,
        (&plugin, "workspace_tree"),
        canonical.clone(),
    )
    .unwrap();
    assert_ne!(first.output.parent(), second.output.parent());
    let (one, two) = std::thread::scope(|scope| {
        let one = scope.spawn(|| first.run());
        let two = scope.spawn(|| second.run());
        (one.join().unwrap(), two.join().unwrap())
    });
    assert_ne!(one.is_ok(), two.is_ok());
    first.record_terminal(&mut session, doc, &one, 0.0).unwrap();
    second
        .record_terminal(&mut session, doc, &two, 0.0)
        .unwrap();
    let (winner, result) = if let Ok(result) = one {
        (&first, result)
    } else {
        (&second, two.unwrap())
    };
    winner.record(&mut session, doc, &result).unwrap();
    assert_eq!(visible_files(root.path()), ["source.nwk", "species.spart"]);
    assert_eq!(
        basecraft_ops::analysis_store::ArtifactRun::for_result(&canonical)
            .unwrap()
            .unwrap(),
        winner.output.parent().unwrap()
    );
    assert_eq!(
        basecraft_ops::files::load_delimitation(&canonical)
            .unwrap()
            .specimens
            .len(),
        4
    );
    for job in [&first, &second] {
        assert!(
            job.output
                .parent()
                .unwrap()
                .join("execution.json")
                .is_file()
        );
    }
}

#[test]
fn optional_display_tree_uses_hidden_reference_first_companions() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(
        &source,
        ">a description\nAAAAA\nAAAAA\n>b\nAAAAAACCAA\n>c\nAAAAGGCCAA\n>d\nAATTGGCCAA\n",
    )
    .unwrap();
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut session = Session::recoverable(root.path()).unwrap();
    let input = session.open(&source).unwrap();
    let canonical = root.path().join("species.spart");
    let setup = FileJob::from_alignment(
        &session,
        input,
        (&plugins.join("delimitation"), "workspace_alignment"),
        basecraft_plugin_host::file_tool::AlignmentInput::Aligned,
        canonical.clone(),
    )
    .unwrap();
    let result = setup.run().unwrap();
    setup.record(&mut session, input, &result).unwrap();
    let workspace = session.open(&canonical).unwrap();
    let display = root.path().join("display.nwk");
    let mut job = FileJob::from_file(
        &session,
        workspace,
        &source,
        (&plugins.join("distance"), "tree"),
        display.clone(),
    )
    .unwrap();
    job.params.extend([
        ("method".into(), json!("upgma")),
        ("model".into(), json!("p")),
        ("min_overlap".into(), json!(1)),
    ]);
    let result = job.run().unwrap();
    job.record(&mut session, workspace, &result).unwrap();
    assert_eq!(
        visible_files(root.path()),
        ["display.nwk", "source.afa", "species.spart"]
    );
    assert_eq!(
        job.input_io().unwrap()["persistent_submitted_copy_bytes"],
        0
    );
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
}
