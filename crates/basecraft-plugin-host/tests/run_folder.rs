//! A run folder records paths relative to itself, so it moves with its project.
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{self, Registry, RegistryConfig},
    runner::recorded_request,
};
use basecraft_protocol::{
    operation::{InputReference, OperationOutcome, OperationRequest, RuntimeState},
    records::RunRecord,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The counting fixture, installed; `edit` may change its contract first.
fn installed(root: &Path, edit: impl FnOnce(&mut Value)) -> Registry {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let package = root.join("package");
    std::fs::create_dir(&package).unwrap();
    for name in ["plugin.toml", "plugin.py"] {
        std::fs::copy(fixture.join(name), package.join(name)).unwrap();
    }
    let mut contract: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("operations.json")).unwrap()).unwrap();
    edit(&mut contract);
    std::fs::write(package.join("operations.json"), contract.to_string()).unwrap();
    let config = RegistryConfig {
        bundled: root.join("absent"),
        packages: root.join("packages"),
        index: root.join("plugins.json"),
    };
    registry::install(&config, &package).unwrap();
    Registry::load(&config).unwrap()
}

/// The fixture installed, and a project folder holding `file` with `bytes`.
fn setup(
    edit: impl FnOnce(&mut Value),
    file: &str,
    bytes: &[u8],
) -> (tempfile::TempDir, Registry, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let registry = installed(temp.path(), edit);
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let project = project.canonicalize().unwrap();
    std::fs::write(project.join(file), bytes).unwrap();
    (temp, registry, project)
}

/// A request to count the bytes of `file`, relative to the project.
fn count(file: &str) -> OperationRequest {
    OperationRequest {
        operation: "org.example.registry-fixture/count@1".into(),
        inputs: [(
            "sources".into(),
            vec![InputReference::File { path: file.into() }],
        )]
        .into(),
        parameters: json!({"mode":"normal"}),
        destinations: std::collections::BTreeMap::new(),
        pins: None,
        request_key: None,
        target: None,
    }
}

fn run(registry: &Registry, project: &Path, request: OperationRequest) -> OperationOutcome {
    let mut jobs = JobManager::default();
    let policy = SessionPolicy::new(project, true).unwrap();
    let accepted = jobs
        .submit(registry, request, policy, "run folder test")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome = jobs.get(&accepted.run_id).unwrap();
        if outcome.state.terminal() {
            assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
            return outcome;
        }
        assert!(Instant::now() < deadline, "{outcome:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn digests(outcome: &OperationOutcome) -> Vec<String> {
    outcome.artifacts.iter().map(|a| a.digest.clone()).collect()
}

#[test]
fn a_moved_run_folder_resolves_and_its_request_runs_again_to_the_same_hashes() {
    let (_temp, registry, project) = setup(|_| {}, "input.txt", b"ab\nc\n");
    let request = count("input.txt");
    let first = run(&registry, &project, request);
    let folder = PathBuf::from(first.evidence.clone().unwrap());
    let inside = folder.strip_prefix(&project).unwrap().to_path_buf();

    // Move the whole project; the run folder goes with it.
    let moved = project.with_file_name("moved");
    std::fs::rename(&project, &moved).unwrap();
    let folder = moved.join(inside);
    let record: RunRecord =
        serde_json::from_slice(&std::fs::read(folder.join("execution.json")).unwrap()).unwrap();
    let text = std::fs::read_to_string(folder.join("execution.json")).unwrap();
    assert!(!text.contains(project.to_str().unwrap()), "{text}");
    let source = &record.inputs["sources"][0];
    let original = folder.join(source.original_path.as_ref().unwrap());
    assert_eq!(std::fs::read(original).unwrap(), b"ab\nc\n");
    assert!(folder.join(&source.run_directory).is_dir());

    let again = run(
        &registry,
        &moved,
        recorded_request(&folder, &registry).unwrap(),
    );
    assert_eq!(digests(&again), digests(&first));

    // A different installed package is refused, not run with other results.
    let other = tempfile::tempdir().unwrap();
    let changed = installed(other.path(), |contract| {
        contract["operations"][0]["description"] = Value::from("Changed");
    });
    let refused = recorded_request(&folder, &changed).unwrap_err().to_string();
    assert!(refused.contains("plugin_changed"), "{refused}");
}

#[test]
fn a_file_run_leaves_one_tool_run_that_points_at_its_run_record() {
    let (_temp, registry, project) = setup(|_| {}, "input.txt", b"abc\n");
    let outcome = run(&registry, &project, count("input.txt"));
    let folder = PathBuf::from(outcome.evidence.clone().unwrap());
    let publication: basecraft_protocol::provenance::Publication =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    let tool_runs: Vec<_> = publication
        .events
        .iter()
        .filter_map(|event| match &event.op {
            basecraft_protocol::Op::ToolRun { params, run, .. } => Some((params, run)),
            _ => None,
        })
        .collect();
    assert_eq!(tool_runs.len(), 1);
    let (params, link) = tool_runs[0];
    // The entry holds the tool's own parameters; the rest is in the run record.
    assert_eq!(*params, count("input.txt").parameters);
    assert_eq!(link.as_ref().unwrap().id, outcome.run_id);
    let record: RunRecord =
        serde_json::from_slice(&std::fs::read(folder.join("execution.json")).unwrap()).unwrap();
    assert_eq!(record.effective_parameters, outcome.effective_parameters);
    assert!(
        record
            .environment
            .interpreter
            .unwrap()
            .starts_with("Python 3.")
    );
}

#[test]
fn an_input_with_the_wrong_alphabet_is_refused_before_launch() {
    let (_temp, registry, project) = setup(
        |contract| {
            contract["operations"][0]["inputs"]["sources"] = json!({
                "view":"aligned_fasta","media_type":"text/x-fasta","min":1,"max":8,
                "alphabets":["dna"]
            });
        },
        "proteins.fasta",
        b">a\nMKWVTFISLLFLFSSAYS\n>b\nMKWVTFISLLFLFSSAYS\n",
    );
    let request = count("proteins.fasta");
    let mut jobs = JobManager::default();
    let policy = SessionPolicy::new(&project, true).unwrap();
    let error = jobs
        .submit(&registry, request, policy, "alphabet test")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "Count bytes needs a DNA alignment, and proteins.fasta holds protein sequences"
        ),
        "{error}"
    );
    assert!(!project.join(".basecraft").exists());
}

/// A cancel request while a caller waits ends the run as cancelled quickly.
#[test]
fn a_cancel_during_a_wait_ends_the_run() {
    let (_temp, registry, project) = setup(|_| {}, "input.txt", b"abc\n");
    let mut request = count("input.txt");
    request.parameters = serde_json::from_str(r#"{"mode":"slow"}"#).unwrap();
    let mut jobs = JobManager::default();
    let policy = SessionPolicy::new(&project, true).unwrap();
    let accepted = jobs
        .submit(&registry, request, policy, "cancel test")
        .unwrap();
    let started = Instant::now();
    let outcome = jobs
        .wait(&accepted.run_id, None, &|| {
            started.elapsed() > Duration::from_millis(300)
        })
        .unwrap();
    assert_eq!(outcome.state, RuntimeState::Cancelled, "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
