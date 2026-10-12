use basecraft_protocol::delimitation::{DelimitationRequest, DelimitationResult, JobStatus};
use std::{collections::BTreeMap, process::Command};

#[test]
fn executable_preserves_success_failure_cancellation_and_hash_contract() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("tree.nwk");
    std::fs::write(&input, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let mut request = DelimitationRequest {
        version: 1,
        job_id: "test".into(),
        generation: 4,
        method: "gmyc_single".into(),
        input: input.display().to_string(),
        input_hash: basecraft_io::hash_file(&input).unwrap(),
        output: directory.path().join("result.json").display().to_string(),
        rooted: true,
        tree_index: 0,
        seed: 1,
        workers: 2,
        parameters: BTreeMap::new(),
        progress: Some(directory.path().join("progress.json").display().to_string()),
        cancel_file: None,
    };
    let request_path = directory.path().join("request.json");
    let execute = |request: &DelimitationRequest| {
        std::fs::write(&request_path, serde_json::to_vec(request).unwrap()).unwrap();
        let process = Command::new(env!("CARGO_BIN_EXE_basecraft-delimitation"))
            .arg("--request")
            .arg(&request_path)
            .output()
            .unwrap();
        let result: DelimitationResult =
            serde_json::from_slice(&std::fs::read(&request.output).unwrap()).unwrap();
        (process, result)
    };
    let (process, result) = execute(&request);
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    assert_eq!(result.job.status, JobStatus::Completed);
    assert_eq!(result.job.generation, 4);
    assert_eq!(result.job.outputs.len(), 2);
    basecraft_docs::delimitation::validate(result.workspace.as_ref().unwrap()).unwrap();
    request.input_hash = "changed".into();
    let (process, result) = execute(&request);
    assert!(!process.status.success());
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(result.job.diagnostics[0].contains("hash differs"));
    let cancel = directory.path().join("cancel");
    std::fs::write(&cancel, "").unwrap();
    request.cancel_file = Some(cancel.display().to_string());
    let (_, result) = execute(&request);
    assert_eq!(result.job.status, JobStatus::Cancelled);
    assert!(result.workspace.is_none());
}
