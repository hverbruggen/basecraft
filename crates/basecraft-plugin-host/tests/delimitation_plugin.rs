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
        let report: Value = serde_json::from_slice(
            &std::fs::read(format!("{}.run.json", output.display())).unwrap(),
        )
        .unwrap();
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
