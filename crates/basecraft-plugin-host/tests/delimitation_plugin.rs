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
        let retained = std::fs::read(run_directory.join("input.snapshot")).unwrap();
        assert_eq!(basecraft_io::hash_bytes(&retained), job.input_key.1);
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
        let outcome = AlignmentJob {
            preparation,
            unique_haplotypes: unique,
            fit_method: "gmyc_single".into(),
            output: output.clone(),
        }
        .run()
        .record(&mut session, doc);
        if unique {
            outcome.unwrap();
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
    assert!(
        AlignmentJob {
            preparation,
            unique_haplotypes: true,
            fit_method: "gmyc_single".into(),
            output: output.clone()
        }
        .run()
        .record(&mut session, single_doc)
        .is_err()
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(format!("{}.run.json", output.display())).unwrap())
            .unwrap();
    assert_eq!(report["job"]["status"], "unavailable");
    assert!(!output.exists());
    drop(session);
    let result = replay::replay(root.path()).unwrap();
    assert!(result.ok(), "{:?}", result.problems);
}
