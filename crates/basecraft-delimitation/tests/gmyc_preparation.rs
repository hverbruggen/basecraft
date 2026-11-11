use basecraft_protocol::{
    delimitation::{DelimitationRequest, DelimitationResult},
    records::JobStatus,
};
use serde_json::json;
use std::{collections::BTreeSet, process::Command};

#[test]
fn zero_age_preparation_restores_tree_tips_then_exact_haplotype_members() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("tree.nwk");
    // a and b differ at an ambiguity: they remain distinct exact haplotypes.
    // Their zero-age clade is a separate, explicitly reported fitting step.
    let newick = "(((a:0,b:0):0,c:0):2,((d:0.1,e:0.1):0.9,(f:0,g:0):1):1);";
    std::fs::write(&input, newick).unwrap();
    let alignment = directory.path().join("retained.fa");
    std::fs::write(
        &alignment,
        ">a\nAAAA\n>a2\naaaa\n>b\nAAAN\n>c\nAAA-\n>d\nCCCC\n>e\nCCCT\n>f\nTTTT\n>g\nTTT?\n",
    )
    .unwrap();
    let mapping =
        json!({"a":["a","a2"],"b":["b"],"c":["c"],"d":["d"],"e":["e"],"f":["f"],"g":["g"]});
    let mut request = DelimitationRequest {
        version: 1,
        job_id: "zero-age".into(),
        generation: 0,
        method: "gmyc_single".into(),
        input: input.display().to_string(),
        input_hash: basecraft_io::hash_file(&input).unwrap(),
        output: directory.path().join("result.json").display().to_string(),
        rooted: true,
        tree_index: 0,
        seed: 1,
        workers: 2,
        progress: None,
        cancel_file: None,
        parameters: [
            ("alignment_snapshot".into(), json!(alignment)),
            (
                "alignment_input_hash".into(),
                json!(basecraft_io::hash_file(&alignment).unwrap()),
            ),
            ("haplotypes".into(), mapping.clone()),
        ]
        .into(),
    };
    let execute = |request: &DelimitationRequest| {
        let path = directory.path().join("request.json");
        std::fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_basecraft-delimitation"))
            .arg("--request")
            .arg(path)
            .output()
            .unwrap();
        let result: DelimitationResult =
            serde_json::from_slice(&std::fs::read(&request.output).unwrap()).unwrap();
        (output, result)
    };
    for method in ["gmyc_single", "gmyc_multiple"] {
        request.method = method.into();
        let (output, result) = execute(&request);
        assert!(output.status.success(), "{:?}", result.job.diagnostics);
        assert_eq!(result.job.status, JobStatus::Completed);
        assert_eq!(
            result.metrics["zero_distance_groups"],
            json!({"a":["a","b","c"],"f":["f","g"]})
        );
        assert!(result.job.diagnostics.iter().any(
            |d| d.contains("zero-age tree clades") && d.contains("no exact sequence identity")
        ));
        let workspace = result.workspace.unwrap();
        basecraft_docs::delimitation::validate(&workspace).unwrap();
        assert_eq!(workspace.specimens.len(), 8);
        // Retain the original input tree, including its zero branches.
        assert_eq!(
            workspace.view.display_tree.as_deref(),
            Some(request.input.as_str())
        );
        assert_eq!(std::fs::read_to_string(&input).unwrap(), newick);
        for hypothesis in &workspace.hypotheses {
            let all: BTreeSet<_> = hypothesis
                .groups
                .iter()
                .flat_map(|g| g.members.iter().cloned())
                .collect();
            assert_eq!(
                all,
                ["a", "a2", "b", "c", "d", "e", "f", "g"]
                    .map(String::from)
                    .into()
            );
            assert_eq!(
                hypothesis
                    .groups
                    .iter()
                    .map(|g| g.members.len())
                    .sum::<usize>(),
                8
            );
            assert!(hypothesis.groups.iter().any(|g| {
                ["a", "a2", "b", "c"]
                    .iter()
                    .all(|id| g.members.contains(*id))
            }));
            assert!(
                hypothesis
                    .groups
                    .iter()
                    .any(|g| g.members.contains("f") && g.members.contains("g"))
            );
            assert_eq!(
                serde_json::to_value(&hypothesis.provenance.haplotypes).unwrap(),
                mapping
            );
            assert_eq!(
                hypothesis.provenance.inputs[&request.input],
                request.input_hash
            );
        }
    }
    // Explicit reference compatibility preprocessing must bypass collapse,
    // retain the same input, and record exactly the three changed event ages.
    request.method = "gmyc_single".into();
    request
        .parameters
        .insert("reference_age_clamp".into(), json!(true));
    let (output, result) = execute(&request);
    assert!(output.status.success(), "{:?}", result.job.diagnostics);
    assert!(!result.metrics.contains_key("zero_distance_groups"));
    assert_eq!(result.metrics["reference_age_clamp"]["changed_nodes"], 3);
    assert_eq!(
        result.workspace.unwrap().view.display_tree.as_deref(),
        Some(request.input.as_str())
    );
    assert_eq!(std::fs::read_to_string(&input).unwrap(), newick);

    // All-zero input is unavailable, never jittered into a completed fit.
    std::fs::write(&input, "((a:0,b:0):0,c:0);").unwrap();
    request.input_hash = basecraft_io::hash_file(&input).unwrap();
    request.parameters.clear();
    let (output, result) = execute(&request);
    assert!(!output.status.success());
    assert_eq!(result.job.status, JobStatus::Failed);
    assert!(result.workspace.is_none());
    assert!(
        result
            .job
            .diagnostics
            .iter()
            .any(|d| d.contains("positive finite root height"))
    );
}

#[test]
fn nested_ties_never_publish_multithreshold_hypotheses_or_exchange_artifacts() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("nested.nwk");
    for (case, newick) in [
        "(((a:0.2,b:0.2):0,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
        "((a:0.2,(b:0.2,c:0.2):0):0.8,(d:0.1,e:0.1):0.9);",
        "((a:1,b:1):0,c:1);",
        "(a:1,(b:1,c:1):0);",
        "(((a:0.2,b:0.2):0.8,c:1):0,d:1);",
        // Explicit compatibility clamping also creates a nested tie here.
        "(((a:0.0000001,b:0.0000001):0.0000001,c:0.0000002):0.9999998,(d:0.1,e:0.1):0.9);",
    ]
    .into_iter()
    .enumerate()
    {
        std::fs::write(&input, newick).unwrap();
        for method in ["gmyc_single", "gmyc_multiple", "gmyc_exhaustive"] {
            let request = DelimitationRequest {
                version: 1,
                job_id: "nested-ties".into(),
                generation: 0,
                method: method.into(),
                input: input.display().to_string(),
                input_hash: basecraft_io::hash_file(&input).unwrap(),
                output: directory
                    .path()
                    .join(format!("{case}-{method}.json"))
                    .display()
                    .to_string(),
                rooted: true,
                tree_index: 0,
                seed: 1,
                workers: 1,
                progress: None,
                cancel_file: None,
                parameters: [("reference_age_clamp".into(), json!(case == 5))].into(),
            };
            let path = directory.path().join("request.json");
            std::fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_basecraft-delimitation"))
                .arg("--request")
                .arg(path)
                .output()
                .unwrap();
            let result: DelimitationResult =
                serde_json::from_slice(&std::fs::read(&request.output).unwrap()).unwrap();
            assert_eq!(std::fs::read_to_string(&input).unwrap(), newick);
            if method == "gmyc_single" {
                assert!(output.status.success(), "{:?}", result.job.diagnostics);
                assert_eq!(result.job.status, JobStatus::Completed);
                assert!(result.workspace.is_some());
            } else {
                assert!(!output.status.success());
                assert_eq!(result.job.status, JobStatus::Failed);
                assert!(result.workspace.is_none());
                assert!(result.job.outputs.is_empty());
                assert!(!result.metrics.contains_key("gmyc"));
                assert!(
                    result
                        .job
                        .diagnostics
                        .iter()
                        .any(|d| d.contains("nested tied branching events")
                            && d.contains("unbounded likelihood"))
                );
                for suffix in [".spart", ".spart.basecraft.json"] {
                    assert!(!std::path::Path::new(&format!("{}{suffix}", request.output)).exists());
                }
            }
        }
    }
}
