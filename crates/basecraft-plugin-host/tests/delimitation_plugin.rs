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
    std::fs::write(
        &source,
        ">a\nACGT\n>b\nACGT\n>c\nACGA\n>d\nTCGA\n>e\nTCGT\n",
    )
    .unwrap();
    // Only the upstream numerical process is replaced. Exercise the real public
    // socket, preparation, adapter, native importer, expansion and replay.
    let plugin = root.path().join("delimitation");
    std::fs::create_dir(&plugin).unwrap();
    let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation");
    for name in ["plugin.toml", "delimitation_plugin.py", "asap_adapter.py"] {
        std::fs::copy(original.join(name), plugin.join(name)).unwrap();
    }
    std::fs::write(plugin.join("asap_worker.py"), r#"
import json, sys
from pathlib import Path
r = json.loads(Path(sys.argv[1]).read_text())
names = [line[1:] for line in Path(r['input']).read_text().splitlines() if line.startswith('>')]
individuals = ''.join('<individual id="'+n+'"/>' for n in names)
members = ''.join('<individual ref="'+n+'"/>' for n in names)
Path(r['out'], 'fixture.spart.xml').write_text('<root><individuals>'+individuals+'</individuals><spartitions><spartition label="fixture" spartitionScore="1"><subsets><subset label="1">'+members+'</subset></subsets></spartition></spartitions></root>')
Path(r['worker_metadata']).write_text(json.dumps({'version':'contract fixture, not ASAP inference'}))
"#).unwrap();
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
        preparation.params.insert("model".into(), json!("p"));
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
        let hypothesis = &workspace["hypotheses"][0];
        assert_eq!(
            hypothesis["groups"][0]["members"],
            json!(["a", "b", "c", "d", "e"])
        );
        assert!(hypothesis["provenance"]["analysis_tree"].is_null());
        assert_eq!(
            hypothesis["provenance"]["parameters"]["deduplicate"],
            unique
        );
        assert_eq!(
            hypothesis["provenance"]["parameters"]["retained_count"],
            if unique { 4 } else { 5 }
        );
        let params = &hypothesis["provenance"]["parameters"];
        let snapshot = params["alignment_snapshot"].as_str().unwrap();
        assert_eq!(
            basecraft_io::hash_file(Path::new(snapshot)).unwrap(),
            params["alignment_input_hash"]
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
