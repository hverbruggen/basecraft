//! Retained registered-run fixture, without invoking external tools.
use basecraft_ops::{analysis_store::ArtifactRun, input_evidence, records};
use serde_json::{Value, json};
use std::path::Path;

pub fn retain(root: &Path, request: &Value, outcome: &mut Value) {
    let reports = root.join(request["destinations"]["reports"].as_str().unwrap());
    let anchor = reports.parent().unwrap().join("anchor");
    let run = ArtifactRun::create(&anchor, &ulid::Ulid::generate().to_string()).unwrap();
    let mut evidence = serde_json::Map::new();
    let mut inputs = vec![];
    let mut first = None;
    let mut upstream = vec![];
    for role in ["sequences", "sources", "features", "template"] {
        let source = root.join(request["inputs"][role][0]["path"].as_str().unwrap());
        let hash = basecraft_io::hash_file(&source).unwrap();
        let ledger =
            input_evidence::reference_input_with_dependencies(&anchor, &source, &hash, &[], "test")
                .unwrap();
        let record = input_evidence::read_input(ledger.directory()).unwrap();
        upstream.push(json!({"bundle":input_evidence::relative(run.directory(),ledger.directory()).unwrap(), "id":ledger.id(),"hash":basecraft_io::hash_file(&ledger.artifact("manifest.json").unwrap()).unwrap()}));
        let mut reference = record.source.clone().unwrap();
        reference.path = input_evidence::relative(run.directory(), &source).unwrap();
        if first.is_none() {
            let mut record = record;
            record.source = Some(reference.clone());
            first = Some(record);
        }
        inputs.push(json!({"id":role,"logical_name":role,"hash":hash,"size":reference.stamp.size,"retained":null,"reference":reference}));
        evidence.insert(role.into(), json!([{"original_path":source,"source_hash":hash,"submitted_hash":hash,"transformation":"identity-v1","run_directory":ledger.directory()}]));
    }
    let mut first = first.unwrap();
    first.upstream = serde_json::from_value(json!(upstream)).unwrap();
    input_evidence::write_json(&run.artifact("input.json").unwrap(), &first).unwrap();
    let mut artifacts = vec![];
    let mut outputs = vec![];
    for entry in std::fs::read_dir(&reports).unwrap() {
        let path = entry.unwrap().path();
        let hash = basecraft_io::hash_file(&path).unwrap();
        artifacts.push(json!({"role":"reports","path":path,"digest":hash}));
        outputs.push(json!({"id":path.file_name().unwrap().to_str().unwrap(),"logical_name":input_evidence::relative(reports.parent().unwrap(),&path).unwrap(),"hash":hash,"size":std::fs::metadata(&path).unwrap().len(),"retained":null}));
    }
    outcome["artifacts"] = json!(artifacts);
    outcome["run_id"] = json!(run.id());
    outcome["operation"] = request["operation"].clone();
    outcome["evidence"] = json!(run.directory());
    outcome["pins"] =
        json!({"package_digest":"test-package", "descriptor_digest":"test-descriptor"});
    outcome["effective_parameters"] = json!({});
    let context = json!({"schema":1,"operation":request["operation"],"run_id":run.id(),"inputs":{},"outputs":{},"cpu_threads":1,"cancel_file":"cancel","progress_file":"progress","input_evidence":evidence});
    input_evidence::write_json(&run.artifact("execution.json").unwrap(), &json!({"schema":1,"operation":request["operation"],"package_digest":"test-package","descriptor_digest":"test-descriptor","effective_parameters":{},"context":context})).unwrap();
    input_evidence::write_json(&run.artifact("request.json").unwrap(), request).unwrap();
    input_evidence::write_json(&run.artifact("runtime.json").unwrap(), outcome).unwrap();
    let publication = serde_json::from_value(json!({"schema_version":1,"id":run.id(),"session":"test","parents":[],"parent_hashes":{},"created":"2026-10-28T00:00:00Z","origin":"test","inputs":inputs,"outputs":outputs,"events":[],"effective_edits":[],"completeness":{"ancestry_known":false,"input_bytes_available":true,"execution_specification_available":true,"replay_verified":false},"report_template_version":2})).unwrap();
    records::publish_operation(run.directory(), &publication).unwrap();
    for artifact in artifacts {
        run.bind(Path::new(artifact["path"].as_str().unwrap()))
            .unwrap();
    }
}
