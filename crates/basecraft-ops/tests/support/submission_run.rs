//! Retained registered-run fixture, without invoking external tools.
use basecraft_ops::{records, records::ArtifactRun};
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
            records::reference_input_with_dependencies(&anchor, &source, &hash, &[], "test", None)
                .unwrap();
        let record = records::read_input(ledger.directory()).unwrap();
        upstream.push(json!({"bundle":records::relative(run.directory(),ledger.directory()).unwrap(), "id":ledger.id(),"hash":basecraft_io::hash_file(&ledger.artifact("manifest.json").unwrap()).unwrap()}));
        let mut reference = record.source.clone().unwrap();
        reference.path = records::relative(run.directory(), &source).unwrap();
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
    records::write_json(&run.artifact("input.json").unwrap(), &first).unwrap();
    let mut artifacts = vec![];
    let mut outputs = vec![];
    for entry in std::fs::read_dir(&reports).unwrap() {
        let path = entry.unwrap().path();
        let hash = basecraft_io::hash_file(&path).unwrap();
        artifacts.push(json!({"role":"reports","path":path,"digest":hash}));
        outputs.push(json!({"id":path.file_name().unwrap().to_str().unwrap(),"logical_name":records::relative(reports.parent().unwrap(),&path).unwrap(),"hash":hash,"size":std::fs::metadata(&path).unwrap().len(),"retained":null}));
    }
    outcome["artifacts"] = json!(artifacts);
    outcome["run_id"] = json!(run.id());
    outcome["operation"] = request["operation"].clone();
    outcome["evidence"] = json!(run.directory());
    outcome["pins"] =
        json!({"package_digest":"test-package", "descriptor_digest":"test-descriptor"});
    outcome["effective_parameters"] = json!({});
    records::write_json(&run.artifact("execution.json").unwrap(), &json!({"schema":2,"operation":request["operation"],"package_version":"1","package_digest":"test-package","descriptor_digest":"test-descriptor","runtime":null,"initiator":"test","executor":"test","process_instance":"test","inputs":evidence,"effective_parameters":{},"cpu_threads":1,"stdout":"","stderr":"","tool":null,"execution":null})).unwrap();
    records::write_json(&run.artifact("request.json").unwrap(), request).unwrap();
    records::write_json(&run.artifact("runtime.json").unwrap(), outcome).unwrap();
    let publication = serde_json::from_value(json!({"schema_version":1,"id":run.id(),"session":"test","parents":[],"parent_hashes":{},"created":"2026-10-28T00:00:00Z","origin":"test","inputs":inputs,"outputs":outputs,"events":[],"effective_edits":[],"completeness":{"ancestry_known":false,"input_bytes_available":true,"execution_specification_available":true,"replay_verified":false},"report_template_version":2})).unwrap();
    records::publish(run.directory(), &publication).unwrap();
    for artifact in artifacts {
        run.bind(Path::new(artifact["path"].as_str().unwrap()))
            .unwrap();
    }
}
