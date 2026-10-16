//! REV-0010 F1: real shared-host cache creation/reuse and publication boundaries.
use std::path::{Path, PathBuf};

use basecraft_ops::{Session, analysis_store::ArtifactRun, provenance};
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
use serde_json::{Value, json};

const INPUT: &str = ">a\nAAAA\n>b\nAAAT\n>c\nAATT\n";

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn make_job(
    session: &Session,
    doc: basecraft_ops::DocId,
    operation: &str,
    output: &Path,
    cache: &Path,
) -> FileJob {
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/distance");
    let mut job = FileJob::from_alignment(
        session,
        doc,
        (&plugin, operation),
        AlignmentInput::Aligned,
        output.into(),
    )
    .unwrap();
    job.params.extend([
        ("model".into(), json!("p")),
        ("threads".into(), json!(1)),
        ("cache".into(), json!(cache)),
    ]);
    job
}

fn run_dir(root: &Path, job: &FileJob) -> PathBuf {
    root.join(".basecraft/analyses-v1/runs")
        .join(job.params["job_id"].as_str().unwrap())
}

#[test]
fn explicit_cache_creation_reuse_mismatch_and_export_retain_checked_evidence() {
    for operation in ["matrix", "tree"] {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("working");
        let exports = parent.path().join("explicit-exports");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&exports).unwrap();
        let source = root.join("source.afa");
        std::fs::write(&source, INPUT).unwrap();
        let mut session = Session::recoverable(&root).unwrap();
        let doc = session.open(&source).unwrap();
        let extension = if operation == "matrix" { "tsv" } else { "nwk" };
        // An explicit cache remains JSON even with a tree-like filename.
        let cache = exports.join("chosen-cache.nwk");
        for (name, reused) in [
            ("first", false),
            ("second", true),
            ("different-model", false),
        ] {
            let output = root.join(format!("{name}.{extension}"));
            let mut job = make_job(&session, doc, operation, &output, &cache);
            if name == "different-model" {
                job.params.insert("model".into(), json!("jc69"));
            }
            let before = std::fs::read(&cache).ok();
            let result = job.run().unwrap();
            job.record(&mut session, doc, &result).unwrap();
            let run = ArtifactRun::for_result(&output).unwrap().unwrap();
            let cache_name = format!("{name}.{extension}.distances.json");
            assert!(run.join(&cache_name).is_file());
            assert_eq!(
                read(&run.join(format!("{name}.{extension}.run.json")))["cache_reused"],
                reused
            );
            assert_eq!(read(&run.join("execution.json"))["status"], "completed");
            assert!(!run.join("submitted.fasta").exists());
            assert_eq!(provenance::verify(&output).unwrap(), Vec::<String>::new());
            let declaration = read(&run.join("cache-publication.json"));
            if let Some(before) = before {
                assert_eq!(std::fs::read(&cache).unwrap(), before);
                assert_eq!(declaration["existing_input"], true);
                let publication = provenance::inspect(&output).unwrap();
                let accepted = publication
                    .inputs
                    .iter()
                    .find(|a| a.logical_name == "distance-cache-input")
                    .unwrap();
                assert_eq!(accepted.hash, basecraft_io::hash_bytes(&before));
                assert_eq!(
                    std::fs::read(run.join(accepted.retained.as_ref().unwrap())).unwrap(),
                    before
                );
                assert!(accepted.reference.is_none());
                assert_eq!(declaration["status"], "retained");
            } else {
                assert_eq!(
                    std::fs::read(&cache).unwrap(),
                    std::fs::read(run.join(&cache_name)).unwrap()
                );
                assert!(result.outputs.iter().any(|p| Path::new(p) == cache));
                assert_eq!(declaration["status"], "published");
            }
            println!(
                "{operation}-{name}: external={:?}, hidden={:?}",
                cache,
                std::fs::read_dir(&run)
                    .unwrap()
                    .map(|e| e.unwrap().file_name())
                    .collect::<Vec<_>>()
            );
        }
        let result = root.join(format!("second.{extension}"));
        let package = parent.path().join("package");
        provenance::package(&result, &package).unwrap();
        std::fs::remove_file(&cache).unwrap();
        assert_eq!(provenance::verify(&result).unwrap(), Vec::<String>::new());
        std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
        assert_eq!(
            provenance::verify(&package.join(result.file_name().unwrap())).unwrap(),
            Vec::<String>::new()
        );
    }
}

#[test]
fn cache_aliases_invalid_inputs_and_destinations_fail_without_replacing_files() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path();
    let source = root.join("source.afa");
    std::fs::write(&source, INPUT).unwrap();
    let mut session = Session::recoverable(root).unwrap();
    let doc = session.open(&source).unwrap();
    let hard = root.join("hard.json");
    std::fs::hard_link(&source, &hard).unwrap();
    let malformed = root.join("unrelated.json");
    std::fs::write(&malformed, "unrelated user bytes").unwrap();
    let mut cases = vec![
        source.clone(),
        hard,
        malformed.clone(),
        root.into(),
        root.join("absent/cache.json"),
    ];
    #[cfg(unix)]
    {
        let linked = root.join("linked.json");
        std::os::unix::fs::symlink(&source, &linked).unwrap();
        cases.push(linked);
        let dangling = root.join("dangling.json");
        std::os::unix::fs::symlink(root.join("absent.json"), &dangling).unwrap();
        cases.push(dangling);
        let linked_parent = root.join("linked-parent");
        std::os::unix::fs::symlink(root, &linked_parent).unwrap();
        cases.push(linked_parent.join("cache.json"));
    }
    for (i, cache) in cases.into_iter().enumerate() {
        let output = root.join(format!("failed-{i}.tsv"));
        let job = make_job(&session, doc, "matrix", &output, &cache);
        let outcome = job.run();
        assert!(outcome.is_err(), "{cache:?}");
        job.record_terminal(&mut session, doc, &outcome, 0.0)
            .unwrap();
        assert!(!output.exists());
        assert!(ArtifactRun::for_result(&output).unwrap().is_none());
        assert_eq!(
            read(&run_dir(root, &job).join("execution.json"))["status"],
            "failed"
        );
    }
    let output = root.join("same.tsv");
    let job = make_job(&session, doc, "matrix", &output, &output);
    assert!(job.run().is_err());
    assert!(!output.exists());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), INPUT);
    assert_eq!(
        std::fs::read_to_string(&malformed).unwrap(),
        "unrelated user bytes"
    );
}

fn fixture(root: &Path) -> PathBuf {
    let plugin = root.join("plugin");
    std::fs::create_dir(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.toml"), "schema = 1\nid = 'basecraft.distance'\nname = 'Cache publication fixture'\nversion = 'test'\n[entry]\noperation = 'python:test.py'\n").unwrap();
    std::fs::write(plugin.join("test.py"), r"
import json, os, subprocess
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('basecraft.distance', 'test')
@plugin.operation('matrix')
def matrix(snapshot, params):
    allocation = Path(params['analysis_artifacts']['metadata_directory'])
    cache = Path(params['cache'])
    assert cache.parent == allocation / 'pending'
    target = Path(params['output'])
    run = subprocess.run([os.environ['BASECRAFT_DISTANCE'], '--input', params['input'], '--matrix', str(target), '--cache', str(cache), '--model', 'p'], capture_output=True, text=True, check=True)
    report = Path(str(target) + '.run.json')
    report.write_text(run.stdout)
    destination = Path(json.loads((allocation / 'cache-publication.json').read_text())['destination'])
    if params.get('unexpected'):
        extra = destination.parent / 'unallocated.json'
        extra.write_text('unrelated')
        return {'ops': [], 'outputs': [str(target), str(cache), str(report), str(extra)]}
    destination.write_text('late competing file')
    return {'ops': [], 'outputs': [str(target), str(cache), str(report)]}
plugin.run()
").unwrap();
    plugin
}

#[test]
fn late_secondary_collision_and_unexpected_return_keep_the_primary_unpublished() {
    for unexpected in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("working");
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.afa");
        std::fs::write(&source, INPUT).unwrap();
        let plugin = fixture(parent.path());
        let output = root.join("matrix.tsv");
        let cache = root.join("chosen-cache.json");
        let mut session = Session::recoverable(&root).unwrap();
        let doc = session.open(&source).unwrap();
        let mut job = FileJob::from_alignment(
            &session,
            doc,
            (&plugin, "matrix"),
            AlignmentInput::Aligned,
            output.clone(),
        )
        .unwrap();
        job.params.extend([
            ("cache".into(), json!(cache)),
            ("unexpected".into(), json!(unexpected)),
        ]);
        let outcome = job.run();
        assert!(outcome.is_err());
        job.record_terminal(&mut session, doc, &outcome, 0.0)
            .unwrap();
        assert!(!output.exists());
        assert!(ArtifactRun::for_result(&output).unwrap().is_none());
        let run = run_dir(&root, &job);
        assert_eq!(read(&run.join("execution.json"))["status"], "failed");
        assert!(run.join("pending/matrix.tsv").is_file());
        if unexpected {
            assert!(!cache.exists());
            assert!(run.join("pending/matrix.tsv.distances.json").is_file());
            assert!(
                outcome
                    .unwrap_err()
                    .to_string()
                    .contains("unallocated artifact")
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(&cache).unwrap(),
                "late competing file"
            );
            assert!(run.join("matrix.tsv.distances.json").is_file());
            assert_eq!(
                read(&run.join("cache-publication.json"))["status"],
                "failed"
            );
            let execution = read(&run.join("execution.json"));
            assert!(
                execution["outputs"]
                    .get(run.join("matrix.tsv.distances.json").to_str().unwrap())
                    .is_some()
            );
        }
    }
}
