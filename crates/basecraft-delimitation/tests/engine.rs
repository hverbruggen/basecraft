use basecraft_protocol::{
    delimitation::{DelimitationRequest, DelimitationResult},
    records::JobStatus,
};
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
    let alignment = directory.path().join("retained.fa");
    std::fs::write(
        &alignment,
        ">a\nACGT\n>a2\nacgu\n>b\nACGA\n>c\nTCGA\n>d\nTCGT\n",
    )
    .unwrap();
    request.parameters.extend([
        ("alignment_snapshot".into(), serde_json::json!(alignment)),
        (
            "alignment_input_hash".into(),
            serde_json::json!(basecraft_io::hash_file(&alignment).unwrap()),
        ),
        (
            "haplotypes".into(),
            serde_json::json!({"a":["a","a2"],"b":["b"],"c":["c"],"d":["d"]}),
        ),
    ]);
    let (process, result) = execute(&request);
    assert!(process.status.success(), "{:?}", result.job.diagnostics);
    let work = result.workspace.unwrap();
    assert_eq!(work.specimens.len(), 5);
    assert_eq!(
        work.hypotheses[0]
            .groups
            .iter()
            .map(|g| g.members.len())
            .sum::<usize>(),
        5
    );
    assert!(
        work.hypotheses[0]
            .provenance
            .inputs
            .contains_key(&alignment.display().to_string())
    );
    request.parameters.insert(
        "haplotypes".into(),
        serde_json::json!({"a":["a","wrong"],"b":["b"],"c":["c"],"d":["d"]}),
    );
    let (_, result) = execute(&request);
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(
        result
            .job
            .diagnostics
            .iter()
            .any(|message| message.contains("differs from retained alignment"))
    );
    std::fs::write(&alignment, ">a\nCCCC\n").unwrap();
    let (_, result) = execute(&request);
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(
        result
            .job
            .diagnostics
            .iter()
            .any(|message| message.contains("Retained alignment changed"))
    );
    request.parameters.clear();
    request
        .parameters
        .insert("haplotypes".into(), serde_json::json!({"wrong":["a","b"]}));
    let (process, result) = execute(&request);
    assert!(!process.status.success());
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(
        result
            .job
            .diagnostics
            .last()
            .unwrap()
            .contains("representatives differ")
    );
    request.parameters.clear();
    request.input_hash = "changed".into();
    let (process, result) = execute(&request);
    assert!(!process.status.success());
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(result.job.diagnostics[0].contains("hash differs"));
    std::fs::write(
        &input,
        include_str!("../../../testdata/fixtures/delimitation/gmyc-coal8.nwk"),
    )
    .unwrap();
    request.input_hash = basecraft_io::hash_file(&input).unwrap();
    request.method = "gmyc_multiple".into();
    request
        .parameters
        .insert("candidate_budget".into(), 1.into());
    let (_, result) = execute(&request);
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(
        result
            .job
            .diagnostics
            .last()
            .unwrap()
            .contains("budget reached")
    );
    request.parameters.clear();
    let cancel = directory.path().join("cancel");
    std::fs::write(&cancel, "").unwrap();
    request.cancel_file = Some(cancel.display().to_string());
    let (_, result) = execute(&request);
    assert_eq!(result.job.status, JobStatus::Cancelled);
    assert!(result.workspace.is_none());
}

#[test]
fn import_expands_unassigned_specimens_and_individual_scores() {
    use serde_json::json;
    use std::collections::BTreeSet;
    let directory = tempfile::tempdir().unwrap();
    let alignment = directory.path().join("original.fa");
    std::fs::write(
        &alignment,
        ">a\nACGT\n>a2\nacgu\n>b\nACGA\n>b2\nacga\n>c\nTCGA\n",
    )
    .unwrap();
    let input = directory.path().join("representatives.spartxml");
    // b is deliberately absent from the subset: it and b2 must stay unassigned.
    std::fs::write(&input, r#"<root><individuals><individual id="a"/><individual id="b"/><individual id="c"/></individuals>
<spartitions><spartition label="contract fixture" individualScoreType="probability"><subsets><subset label="group 1">
<individual ref="a" score="0.75"/><individual ref="c" score="0.25"/>
</subset></subsets></spartition></spartitions></root>"#).unwrap();
    let mapping = json!({"a":["a","a2"],"b":["b","b2"],"c":["c"]});
    let request = DelimitationRequest {
        version: 1,
        job_id: "expansion".into(),
        generation: 0,
        method: "asap_import".into(),
        input: input.display().to_string(),
        input_hash: basecraft_io::hash_file(&input).unwrap(),
        output: directory.path().join("result.json").display().to_string(),
        rooted: false,
        tree_index: 0,
        seed: 1,
        workers: 1,
        progress: None,
        cancel_file: None,
        parameters: [
            ("deduplicate".into(), json!(true)),
            ("alignment_snapshot".into(), json!(alignment)),
            (
                "alignment_input_hash".into(),
                json!(basecraft_io::hash_file(&alignment).unwrap()),
            ),
            ("original_source".into(), json!(alignment)),
            ("haplotypes".into(), mapping.clone()),
        ]
        .into(),
    };
    let request_path = directory.path().join("request.json");
    std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let process = Command::new(env!("CARGO_BIN_EXE_basecraft-delimitation"))
        .arg("--request")
        .arg(&request_path)
        .output()
        .unwrap();
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    let result: DelimitationResult =
        serde_json::from_slice(&std::fs::read(&request.output).unwrap()).unwrap();
    assert_eq!(result.job.status, JobStatus::Completed);
    let workspace = result.workspace.unwrap();
    basecraft_docs::delimitation::validate(&workspace).unwrap();
    let hypothesis = &workspace.hypotheses[0];
    assert_eq!(hypothesis.unassigned, ["b".into(), "b2".into()].into());
    let group = &hypothesis.groups[0];
    assert_eq!(group.members, ["a".into(), "a2".into(), "c".into()].into());
    assert_eq!(group.individual_scores.len(), 3);
    assert_eq!(group.individual_scores["a"], group.individual_scores["a2"]);
    assert_eq!(group.individual_scores["a"].len(), 1);
    assert_eq!(group.individual_scores["a"][0].value, 0.75);
    assert_eq!(group.individual_scores["c"][0].value, 0.25);
    assert!(!group.individual_scores.contains_key("b"));
    let expected: BTreeSet<String> = ["a", "a2", "b", "b2", "c"].map(str::to_string).into();
    assert_eq!(
        group
            .members
            .union(&hypothesis.unassigned)
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert!(group.members.is_disjoint(&hypothesis.unassigned));
    assert_eq!(workspace.specimens.len(), 5);
    assert_eq!(
        workspace
            .specimens
            .iter()
            .map(|specimen| specimen.id.clone())
            .collect::<BTreeSet<_>>(),
        expected
    );
    for specimen in &workspace.specimens {
        assert_eq!(specimen.label, specimen.id);
        assert_eq!(
            specimen.sources[&alignment.display().to_string()],
            specimen.id
        );
    }
    assert_eq!(
        serde_json::to_value(&hypothesis.provenance.haplotypes).unwrap(),
        mapping
    );
    let sidecar: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!("{}.spart.basecraft.json", request.output)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        sidecar["workspace"],
        serde_json::to_value(workspace).unwrap()
    );
}

#[test]
fn native_asap_reproduces_the_reference_best_partition() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fixtures/asap");
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("mantidactylus.fasta");
    std::fs::copy(format!("{fixture}/mantidactylus.fasta"), &input).unwrap();
    let request = DelimitationRequest {
        version: 1,
        job_id: "asap".into(),
        generation: 0,
        method: "asap".into(),
        input: input.display().to_string(),
        input_hash: basecraft_io::hash_file(&input).unwrap(),
        output: directory.path().join("result.json").display().to_string(),
        rooted: false,
        tree_index: 0,
        seed: 1,
        workers: 2,
        progress: Some(directory.path().join("progress.json").display().to_string()),
        cancel_file: None,
        parameters: [
            ("model".into(), serde_json::json!("jc69")),
            ("stream".into(), serde_json::json!("reference")),
        ]
        .into(),
    };
    let request_path = directory.path().join("request.json");
    std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let process = Command::new(env!("CARGO_BIN_EXE_basecraft-delimitation"))
        .arg("--request")
        .arg(&request_path)
        .output()
        .unwrap();
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    let result: DelimitationResult =
        serde_json::from_slice(&std::fs::read(&request.output).unwrap()).unwrap();
    assert_eq!(result.job.status, JobStatus::Completed);
    let workspace = result.workspace.unwrap();
    basecraft_docs::delimitation::validate(&workspace).unwrap();
    assert_eq!(workspace.specimens.len(), 19);
    // Reference ASAP lists 9 candidate partitions; its best has 4 subsets
    // and score 1 (testdata/fixtures/asap/basecraft-distances.partitions.tsv).
    assert_eq!(workspace.hypotheses.len(), 9);
    let best = &workspace.hypotheses[0];
    assert_eq!(best.groups.len(), 4);
    assert_eq!(best.scores[0].value, 1.0);
    assert_eq!(best.attributes["asap_rank"], "1");
    assert_eq!(workspace.view.visible.len(), 9);
    for hypothesis in &workspace.hypotheses {
        let members: usize = hypothesis.groups.iter().map(|g| g.members.len()).sum();
        assert_eq!(members, 19, "{}", hypothesis.name);
    }
    assert!(std::path::Path::new(&format!("{}.asap.tsv", request.output)).is_file());
    assert_eq!(best.provenance.parameters["stream"], "reference");
    assert_eq!(best.provenance.workers, Some(2));
}
