//! Phase A1: original core owners, before moving the host serde declaration.
//! Adapter/Session/runner/Sanger qualification is a separately reserved phase A2.
use basecraft_ops::{
    Error,
    analysis_store::ArtifactRun,
    input_evidence::{self as evidence, AffectedAnalysis, EvidenceConflict, SavePolicy},
};
use basecraft_protocol::provenance::{FileStamp, InputReference, Publication};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

// Frozen original declaration (including order and flatten), not a Value oracle.
mod original {
    use super::{BTreeMap, Deserialize, InputReference, Serialize};
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SubmittedInput {
        pub schema_version: u32,
        pub source: Option<InputReference>,
        pub submitted_hash: String,
        pub submitted_size: u64,
        /// canonical-fasta-v1 or ungapped-fasta-v1; generated inputs retain their bytes.
        pub transformation: String,
        pub state: String,
        #[serde(flatten)]
        pub extra: BTreeMap<String, serde_json::Value>,
    }
}
use original::SubmittedInput as OldSubmittedInput;
fn bytes(input: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(input).unwrap();
    bytes.push(b'\n');
    bytes
}
fn retain(name: &str, data: &[u8]) {
    if let Some(root) = std::env::var_os("BASECRAFT_SUBMITTED_CAPTURE_DIR") {
        fs::create_dir_all(&root).unwrap();
        fs::write(Path::new(&root).join(name), data).unwrap();
    }
}
fn capture(name: &str, run: &Path, expected: &OldSubmittedInput) {
    let actual = fs::read(run.join("input.json")).unwrap();
    assert_eq!(
        actual,
        bytes(expected),
        "original struct byte oracle: {name}"
    );
    retain(&format!("submitted-input-{name}.json"), &actual);
    retain(
        &format!("submitted-input-{name}.oracle.json"),
        &bytes(expected),
    );
    retain(
        &format!("{name}.inputs.json"),
        &bytes(&json!({"run":run,"expected":expected})),
    );
}
// Native metadata read independently of input.json and the production stamp helper.
fn native_stamp(path: &Path) -> FileStamp {
    let m = fs::metadata(path).unwrap();
    let time = m
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    #[cfg(unix)]
    let (device, inode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(m.dev()), Some(m.ino()))
    };
    #[cfg(windows)]
    let (device, inode) = {
        let (d, i) = basecraft_platform::files::identity(path).unwrap();
        (Some(d), Some(i))
    };
    FileStamp {
        size: m.len(),
        modified_seconds: time.as_secs(),
        modified_nanos: time.subsec_nanos(),
        device,
        inode,
    }
}
const SOURCE: &[u8] = b">sample\nACGT\n";
const PAYLOAD: &[u8] = b">generated\nAGT\n";
fn reference(root: &Path, dependencies: &[PathBuf]) -> (ArtifactRun, OldSubmittedInput) {
    let source = root.join("source.fasta");
    let hash = basecraft_io::hash_bytes(SOURCE);
    let stamp = native_stamp(&source);
    let run = evidence::reference_input_with_dependencies(
        &root.join("result.txt"),
        &source,
        &hash,
        dependencies,
        "submitted-input core capture",
    )
    .unwrap();
    assert_eq!(run.directory().ancestors().nth(4).unwrap(), root);
    let mut expected = OldSubmittedInput {
        schema_version: 1,
        source: Some(InputReference {
            path: "../../../../source.fasta".into(),
            stamp,
            hash: hash.clone(),
            extra: BTreeMap::new(),
        }),
        submitted_hash: hash,
        submitted_size: SOURCE.len() as u64,
        transformation: "identity-v1".into(),
        state: "referenced".into(),
        extra: BTreeMap::new(),
    };
    // The public reference owner publishes an empty-event Publication. Its
    // unchanged link_selected_inputs pass rewrites these three extras even
    // without selected dependencies; derive them from that declared empty graph.
    expected
        .extra
        .insert("sanger_input_events".into(), json!([]));
    expected
        .extra
        .insert("selected_input_links".into(), json!({}));
    expected.extra.insert("upstream".into(), json!([]));
    if !dependencies.is_empty() {
        expected
            .extra
            .insert("upstream".into(), upstream(&run, dependencies));
    }
    assert_eq!(fs::read(&source).unwrap(), SOURCE);
    (run, expected)
}
fn upstream(run: &ArtifactRun, deps: &[PathBuf]) -> Value {
    json!(
        deps.iter()
            .map(|d| {
                assert_eq!(run.directory().parent(), d.parent());
                let raw = fs::read(d.join("manifest.json")).unwrap();
                let p: Publication = serde_json::from_slice(&raw).unwrap();
                let id = d.file_name().unwrap().to_str().unwrap();
                assert_eq!(p.id, id);
                retain(&format!("dependency-{id}.manifest.json"), &raw);
                json!({"bundle":format!("../{id}"),"id":id,"hash":basecraft_io::hash_bytes(&raw)})
            })
            .collect::<Vec<_>>()
    )
}
fn affected(run: &ArtifactRun) -> AffectedAnalysis {
    AffectedAnalysis {
        run: run.directory().into(),
        result: "Synthetic analysis".into(),
        source_hash: basecraft_io::hash_bytes(SOURCE),
        can_preserve: true,
        direct: true,
        backing_analyses: vec![],
    }
}
fn conflict(root: &Path, run: &ArtifactRun) -> EvidenceConflict {
    EvidenceConflict {
        target: root.join("source.fasta"),
        analyses: vec![affected(run)],
        options: vec![
            SavePolicy::Cancel,
            SavePolicy::PreserveCopy,
            SavePolicy::BreakEvidence,
            SavePolicy::RemoveAnalysis,
        ],
    }
}
#[test]
fn original_core_writers_and_transitions() {
    let Some(root) = isolated_profile::enter("original_core_writers_and_transitions") else {
        return;
    };
    fs::write(root.join("source.fasta"), SOURCE).unwrap();
    retain("declared-source.fasta", SOURCE);
    retain("declared-generated.fasta", PAYLOAD);
    let (owner, mut old) = reference(&root, &[]);
    capture("referenced", owner.directory(), &old);
    let (dependent, dependent_old) = reference(&root, &[owner.directory().into()]);
    capture("referenced-upstream", dependent.directory(), &dependent_old);
    for (name, deps) in [
        ("generated", vec![]),
        ("generated-upstream", vec![owner.directory().into()]),
    ] {
        let run = ArtifactRun::create(&root.join("result.txt"), name).unwrap();
        // Public contract requires caller-retained generated payload, no engine.
        fs::write(run.artifact("submitted.fasta").unwrap(), PAYLOAD).unwrap();
        evidence::link_generated_input(
            &run,
            &deps,
            PAYLOAD,
            "canonical-fasta-v1",
            "Synthetic generated",
        )
        .unwrap();
        let expected = OldSubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: basecraft_io::hash_bytes(PAYLOAD),
            submitted_size: PAYLOAD.len() as u64,
            transformation: "canonical-fasta-v1".into(),
            state: "generated".into(),
            extra: BTreeMap::from([("upstream".into(), upstream(&run, &deps))]),
        };
        capture(name, run.directory(), &expected);
        assert_eq!(
            fs::read(run.directory().join("submitted.fasta")).unwrap(),
            PAYLOAD
        );
    }
    let before = fs::read(owner.directory().join("input.json")).unwrap();
    retain("cancel.before.json", &before);
    let e = evidence::resolve(&conflict(&root, &owner), SavePolicy::Cancel).unwrap_err();
    assert!(matches!(e, Error::EvidenceConflict(_)));
    assert_eq!(e.code(), "evidence_conflict");
    assert_eq!(
        fs::read(owner.directory().join("input.json")).unwrap(),
        before
    );
    assert_eq!(fs::read(root.join("source.fasta")).unwrap(), SOURCE);
    let (delegate, mut delegate_old) = reference(&root, &[]);
    let mut group = conflict(&root, &delegate);
    group.analyses[0].backing_analyses = vec![affected(&delegate), affected(&owner)];
    evidence::resolve(&group, SavePolicy::PreserveCopy).unwrap();
    old.state = "preserved".into();
    old.extra
        .insert("preserved_original_bytes".into(), json!(SOURCE.len()));
    capture("preserved", owner.directory(), &old);
    delegate_old.state = "preserved".into();
    delegate_old
        .extra
        .insert("preserved_original_bytes".into(), json!(0));
    delegate_old
        .extra
        .insert("origin_run".into(), json!(format!("../{}", owner.id())));
    capture("preserved-delegated", delegate.directory(), &delegate_old);
    assert_eq!(
        fs::read(owner.directory().join("preserved-input")).unwrap(),
        SOURCE
    );
    assert!(!delegate.directory().join("preserved-input").exists());
    evidence::preserve_input(delegate.directory()).unwrap();
    capture("preserved-delegated", delegate.directory(), &delegate_old);
    evidence::resolve(&conflict(&root, &dependent), SavePolicy::BreakEvidence).unwrap();
    let mut broken = dependent_old;
    broken.state = "broken".into();
    capture("broken", dependent.directory(), &broken);
    let moved = root.join("moved.fasta");
    fs::rename(root.join("source.fasta"), &moved).unwrap();
    let moved_stamp = native_stamp(&moved);
    evidence::relink(dependent.directory(), &moved).unwrap();
    broken.state = "referenced".into();
    broken.source.as_mut().unwrap().path = "../../../../moved.fasta".into();
    broken.source.as_mut().unwrap().stamp = moved_stamp.clone();
    capture("relinked", dependent.directory(), &broken);
    // Relink propagates into the declared upstream owner; delegating sibling is untouched.
    old.state = "referenced".into();
    old.source.as_mut().unwrap().path = "../../../../moved.fasta".into();
    old.source.as_mut().unwrap().stamp = moved_stamp;
    assert_eq!(
        fs::read(owner.directory().join("input.json")).unwrap(),
        bytes(&old)
    );
    assert_eq!(
        fs::read(delegate.directory().join("input.json")).unwrap(),
        bytes(&delegate_old)
    );
    assert_eq!(fs::read(&moved).unwrap(), SOURCE);
    let before = fs::read(dependent.directory().join("input.json")).unwrap();
    retain("remove.before.json", &before);
    evidence::resolve(&conflict(&root, &dependent), SavePolicy::RemoveAnalysis).unwrap();
    assert!(!dependent.directory().exists());
    let trash = root.join(".basecraft/analyses-v1/trash");
    let moved_runs: Vec<_> = fs::read_dir(&trash)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(moved_runs.len(), 1);
    let after = fs::read(moved_runs[0].join("input.json")).unwrap();
    assert_eq!(before, after);
    retain("remove.moved.json", &after);
    assert_eq!(fs::read(&moved).unwrap(), SOURCE);
}

#[test]
fn committed_raw_original_records_roundtrip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for name in [
        "referenced",
        "referenced-upstream",
        "generated",
        "generated-upstream",
        "preserved",
        "preserved-delegated",
        "broken",
        "relinked",
    ] {
        let raw = fs::read(root.join(format!("submitted-input-{name}.json"))).unwrap();
        let old: OldSubmittedInput = serde_json::from_slice(&raw).unwrap();
        let host: evidence::SubmittedInput = serde_json::from_slice(&raw).unwrap();
        assert_eq!(bytes(&old), raw);
        assert_eq!(bytes(&host), raw);
    }
}

fn json_error(e: &serde_json::Error) -> Value {
    json!({"variant":"Json","code":"json","display":e.to_string(),"line":e.line(),"column":e.column(),"category":format!("{:?}",e.classify())})
}
fn error(e: &Error) -> Value {
    match e {
        Error::Json(e) => json_error(e),
        Error::Invalid(s) => {
            json!({"variant":"Invalid","code":e.code(),"display":e.to_string(),"detail":s})
        }
        Error::Io(io) => {
            json!({"variant":"Io","code":e.code(),"kind":format!("{:?}",io.kind()),"display":e.to_string()})
        }
        Error::Format(basecraft_io::Error::Io(io)) => {
            json!({"variant":"Format(Io)","code":e.code(),"kind":format!("{:?}",io.kind()),"display":e.to_string()})
        }
        Error::Format(inner) => {
            json!({"variant":"Format","code":e.code(),"display":inner.to_string()})
        }
        _ => panic!("unallocated error variant {e:?}"),
    }
}
fn example() -> Value {
    // Deliberately inconsistent size/hash/path facts are accepted by this reader.
    json!({"schema_version":1,"source":{"path":"missing/原本.fasta","stamp":{"size":7,"modified_seconds":0,"modified_nanos":4294967295u32},"hash":"not-a-hash"},"submitted_hash":"","submitted_size":9,"transformation":"identity-v1","state":"referenced"})
}
fn reader_cases() -> Vec<(String, String)> {
    let mut cases = vec![];
    let mut add = |name: String, v: Value| cases.push((name, serde_json::to_string(&v).unwrap()));
    for state in [
        "referenced",
        "generated",
        "preserved",
        "broken",
        "future",
        "",
        "Referenced",
    ] {
        for source in ["absent", "null", "some"] {
            let mut v = example();
            v["state"] = json!(state);
            match source {
                "absent" => {
                    v.as_object_mut().unwrap().remove("source");
                }
                "null" => v["source"] = Value::Null,
                _ => {}
            }
            add(format!("state/{state}/{source}"), v);
        }
    }
    for t in [
        "canonical-fasta-v1",
        "ungapped-fasta-v1",
        "canonical-newick-v1",
        "identity-v1",
        "blast-query-preparation-v1",
        "sequence-arrow-buffers-v1",
        "unknown",
        "",
        "Identity-v1",
    ] {
        let mut v = example();
        v["transformation"] = json!(t);
        add(format!("transform/{t}"), v);
    }
    for version in [0u64, 2, 4294967295] {
        for state in ["referenced", "generated", "future"] {
            let mut v = example();
            v["schema_version"] = json!(version);
            v["state"] = json!(state);
            v["transformation"] = json!("unknown");
            add(format!("precedence/{version}/{state}"), v);
        }
    }
    for (label, field, value) in [
        ("bad-source-type", "source", json!(true)),
        ("bad-hash-type", "submitted_hash", json!([])),
    ] {
        let mut v = example();
        v["schema_version"] = json!(2);
        v["state"] = json!("future");
        v[field] = value;
        add(format!("precedence/serde-before-version/{label}"), v);
    }
    let mut v = example();
    v["schema_version"] = json!(2);
    v.as_object_mut().unwrap().remove("transformation");
    add("precedence/missing-before-version".into(), v);
    for field in [
        "schema_version",
        "source",
        "submitted_hash",
        "submitted_size",
        "transformation",
        "state",
    ] {
        let mut v = example();
        v.as_object_mut().unwrap().remove(field);
        add(format!("missing/{field}"), v);
        for (label, value) in [
            ("null", Value::Null),
            ("bool", json!(true)),
            ("array", json!([])),
            ("object", json!({})),
            ("negative", json!(-1)),
            ("float", json!(1.5)),
            ("string", json!("x")),
            ("u64-max", json!(u64::MAX)),
        ] {
            let mut v = example();
            v[field] = value;
            add(format!("type/{field}/{label}"), v);
        }
    }
    for field in [
        "size",
        "modified_seconds",
        "modified_nanos",
        "device",
        "inode",
    ] {
        for (label, value) in [
            ("null", Value::Null),
            ("negative", json!(-1)),
            ("bool", json!(true)),
            ("array", json!([])),
            ("object", json!({})),
            ("float", json!(1.5)),
            ("string", json!("x")),
            ("overflow-u32", json!(4294967296u64)),
            ("u64-max", json!(u64::MAX)),
        ] {
            let mut v = example();
            v["source"]["stamp"][field] = value;
            add(format!("stamp/{field}/{label}"), v);
        }
        let mut v = example();
        v["source"]["stamp"].as_object_mut().unwrap().remove(field);
        add(format!("stamp/{field}/absent"), v);
    }
    for field in ["path", "stamp", "hash"] {
        let mut v = example();
        v["source"].as_object_mut().unwrap().remove(field);
        add(format!("reference/{field}/absent"), v);
        let mut v = example();
        v["source"][field] = Value::Null;
        add(format!("reference/{field}/null"), v);
    }
    let mut v = example();
    v["z_extra"] = json!({"null":null,"nested":["雪",{"x":false}]});
    v["source"]["reference_extra"] = json!([null, "é"]);
    v["source"]["stamp"]["stamp_extra"] = json!({"lost":true});
    add("unknown/nested".into(), v);
    let mut v = example();
    v["submitted_size"] = json!(0);
    v["source"]["path"] = json!("");
    v["source"]["hash"] = json!("");
    add("semantic/unchecked-empty".into(), v);
    let base = serde_json::to_string(&example()).unwrap();
    for field in [
        "schema_version",
        "source",
        "submitted_hash",
        "submitted_size",
        "transformation",
        "state",
    ] {
        let value = serde_json::to_string(&example()[field]).unwrap();
        cases.push((
            format!("duplicate/{field}"),
            format!("{{\"{field}\":{value},{}", &base[1..]),
        ));
    }
    cases.push((
        "duplicate/unknown".into(),
        format!("{{\"extra\":1,\"extra\":{{\"last\":null}},{}", &base[1..]),
    ));
    cases.push((
        "duplicate/reference-path".into(),
        base.replace("\"path\":", "\"path\":\"first\",\"path\":"),
    ));
    cases.push((
        "duplicate/stamp-size".into(),
        base.replace("\"size\":7", "\"size\":1,\"size\":7"),
    ));
    cases.push((
        "duplicate/reference-extra".into(),
        base.replace("\"path\":", "\"extra\":1,\"extra\":null,\"path\":"),
    ));
    cases.push((
        "duplicate/stamp-unknown".into(),
        base.replace("\"size\":7", "\"extra\":1,\"extra\":null,\"size\":7"),
    ));
    for (field, prior) in [("schema_version", "1"), ("submitted_size", "9")] {
        cases.push((
            format!("overflow/{field}"),
            base.replace(
                &format!("\"{field}\":{prior}"),
                &format!("\"{field}\":18446744073709551616"),
            ),
        ));
    }
    let mut v = example();
    v["source"] = json!(["x",{"size":0,"modified_seconds":0,"modified_nanos":0},"h"]);
    cases.push((
        "sequence/reference".into(),
        serde_json::to_string(&v).unwrap(),
    ));
    for stamp in [
        json!([7, 0, 0]),
        json!([7, 0, 0, null, null]),
        json!([7, 0, 0, null, null, 1]),
    ] {
        let mut v = example();
        v["source"]["stamp"] = stamp;
        cases.push((
            format!("sequence/stamp/{}", cases.len()),
            serde_json::to_string(&v).unwrap(),
        ));
    }
    for input in [
        "[]",
        "[1,null,\"h\",0,\"identity-v1\",\"generated\"]",
        "[1,null,\"h\",0,\"identity-v1\",\"generated\",{}]",
        "null",
        "true",
        "1",
        "{}",
        "{",
        "{\"schema_version\":18446744073709551616}",
        "{\"submitted_size\":18446744073709551616}",
    ] {
        cases.push((format!("syntax/{}", cases.len()), input.into()));
    }
    cases
}
// Filled once from the ORIGINAL readers; input strings and full observed diagnostics
// are embedded here because this allocation reserves only eight writer fixtures.
const READER_OBSERVATIONS: &str = r###"[
{"case":"state/referenced/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/referenced/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/referenced/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/generated/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":null,"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/generated/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":null,"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/generated/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/preserved/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"preserved","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"preserved\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/preserved/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"preserved","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"preserved\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/preserved/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"preserved","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"preserved","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"preserved\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/broken/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":null,"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"state\":\"broken\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/broken/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":null,"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"broken\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/broken/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"broken","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"broken\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/future/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"future","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/future/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"future","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/future/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"future","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state//absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state//null","direct":{"accepted":{"schema_version":1,"source":null,"state":"","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state//some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/Referenced/absent","direct":{"accepted":{"schema_version":1,"source":null,"state":"Referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"Referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/Referenced/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"Referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"Referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"state/Referenced/some","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"Referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"Referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"transform/canonical-fasta-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"canonical-fasta-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"canonical-fasta-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"canonical-fasta-v1\"}"},
{"case":"transform/ungapped-fasta-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"ungapped-fasta-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"ungapped-fasta-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"ungapped-fasta-v1\"}"},
{"case":"transform/canonical-newick-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"canonical-newick-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"canonical-newick-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"canonical-newick-v1\"}"},
{"case":"transform/identity-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"transform/blast-query-preparation-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"blast-query-preparation-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"blast-query-preparation-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"blast-query-preparation-v1\"}"},
{"case":"transform/sequence-arrow-buffers-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"sequence-arrow-buffers-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"sequence-arrow-buffers-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"sequence-arrow-buffers-v1\"}"},
{"case":"transform/unknown","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"transform/","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":""}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"\"}"},
{"case":"transform/Identity-v1","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"Identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"Identity-v1\"}"},
{"case":"precedence/0/referenced","direct":{"accepted":{"schema_version":0,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":0,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/0/generated","direct":{"accepted":{"schema_version":0,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":0,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/0/future","direct":{"accepted":{"schema_version":0,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"future","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":0,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/2/referenced","direct":{"accepted":{"schema_version":2,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":2,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/2/generated","direct":{"accepted":{"schema_version":2,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":2,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/2/future","direct":{"accepted":{"schema_version":2,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"future","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":2,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/4294967295/referenced","direct":{"accepted":{"schema_version":4294967295,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":4294967295,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/4294967295/generated","direct":{"accepted":{"schema_version":4294967295,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"generated","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":4294967295,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"generated\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/4294967295/future","direct":{"accepted":{"schema_version":4294967295,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"future","submitted_hash":"","submitted_size":9,"transformation":"unknown"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":4294967295,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"unknown\"}"},
{"case":"precedence/serde-before-version/bad-source-type","direct":{"category":"Data","code":"json","column":33,"display":"invalid type: boolean `true`, expected struct InputReference at line 1 column 33","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":33,"display":"invalid type: boolean `true`, expected struct InputReference at line 1 column 33","line":1,"variant":"Json"},"input":"{\"schema_version\":2,\"source\":true,\"state\":\"future\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"precedence/serde-before-version/bad-hash-type","direct":{"category":"Data","code":"json","column":183,"display":"invalid type: sequence, expected a string at line 1 column 183","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":183,"display":"invalid type: sequence, expected a string at line 1 column 183","line":1,"variant":"Json"},"input":"{\"schema_version\":2,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"future\",\"submitted_hash\":[],\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"precedence/missing-before-version","direct":{"category":"Data","code":"json","column":209,"display":"missing field `transformation` at line 1 column 209","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":209,"display":"missing field `transformation` at line 1 column 209","line":1,"variant":"Json"},"input":"{\"schema_version\":2,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9}"},
{"case":"missing/schema_version","direct":{"category":"Data","code":"json","column":221,"display":"missing field `schema_version` at line 1 column 221","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":221,"display":"missing field `schema_version` at line 1 column 221","line":1,"variant":"Json"},"input":"{\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/null","direct":{"category":"Data","code":"json","column":22,"display":"invalid type: null, expected u32 at line 1 column 22","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":22,"display":"invalid type: null, expected u32 at line 1 column 22","line":1,"variant":"Json"},"input":"{\"schema_version\":null,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/bool","direct":{"category":"Data","code":"json","column":22,"display":"invalid type: boolean `true`, expected u32 at line 1 column 22","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":22,"display":"invalid type: boolean `true`, expected u32 at line 1 column 22","line":1,"variant":"Json"},"input":"{\"schema_version\":true,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/array","direct":{"category":"Data","code":"json","column":18,"display":"invalid type: sequence, expected u32 at line 1 column 18","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":18,"display":"invalid type: sequence, expected u32 at line 1 column 18","line":1,"variant":"Json"},"input":"{\"schema_version\":[],\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/object","direct":{"category":"Data","code":"json","column":18,"display":"invalid type: map, expected u32 at line 1 column 18","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":18,"display":"invalid type: map, expected u32 at line 1 column 18","line":1,"variant":"Json"},"input":"{\"schema_version\":{},\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/negative","direct":{"category":"Data","code":"json","column":20,"display":"invalid value: integer `-1`, expected u32 at line 1 column 20","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":20,"display":"invalid value: integer `-1`, expected u32 at line 1 column 20","line":1,"variant":"Json"},"input":"{\"schema_version\":-1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/float","direct":{"category":"Data","code":"json","column":21,"display":"invalid type: floating point `1.5`, expected u32 at line 1 column 21","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":21,"display":"invalid type: floating point `1.5`, expected u32 at line 1 column 21","line":1,"variant":"Json"},"input":"{\"schema_version\":1.5,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/string","direct":{"category":"Data","code":"json","column":21,"display":"invalid type: string \"x\", expected u32 at line 1 column 21","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":21,"display":"invalid type: string \"x\", expected u32 at line 1 column 21","line":1,"variant":"Json"},"input":"{\"schema_version\":\"x\",\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/schema_version/u64-max","direct":{"category":"Data","code":"json","column":38,"display":"invalid value: integer `18446744073709551615`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":38,"display":"invalid value: integer `18446744073709551615`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"input":"{\"schema_version\":18446744073709551615,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"missing/source","direct":{"accepted":{"schema_version":1,"source":null,"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/null","direct":{"accepted":{"schema_version":1,"source":null,"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":null,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/bool","direct":{"category":"Data","code":"json","column":33,"display":"invalid type: boolean `true`, expected struct InputReference at line 1 column 33","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":33,"display":"invalid type: boolean `true`, expected struct InputReference at line 1 column 33","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":true,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/array","direct":{"category":"Data","code":"json","column":29,"display":"invalid type: sequence, expected struct InputReference at line 1 column 29","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":29,"display":"invalid type: sequence, expected struct InputReference at line 1 column 29","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":[],\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/object","direct":{"category":"Data","code":"json","column":31,"display":"missing field `path` at line 1 column 31","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":31,"display":"missing field `path` at line 1 column 31","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/negative","direct":{"category":"Data","code":"json","column":31,"display":"invalid type: integer `-1`, expected struct InputReference at line 1 column 31","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":31,"display":"invalid type: integer `-1`, expected struct InputReference at line 1 column 31","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":-1,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/float","direct":{"category":"Data","code":"json","column":32,"display":"invalid type: floating point `1.5`, expected struct InputReference at line 1 column 32","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":32,"display":"invalid type: floating point `1.5`, expected struct InputReference at line 1 column 32","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":1.5,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/string","direct":{"category":"Data","code":"json","column":32,"display":"invalid type: string \"x\", expected struct InputReference at line 1 column 32","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":32,"display":"invalid type: string \"x\", expected struct InputReference at line 1 column 32","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":\"x\",\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/source/u64-max","direct":{"category":"Data","code":"json","column":49,"display":"invalid type: integer `18446744073709551615`, expected struct InputReference at line 1 column 49","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":49,"display":"invalid type: integer `18446744073709551615`, expected struct InputReference at line 1 column 49","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":18446744073709551615,\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"missing/submitted_hash","direct":{"category":"Data","code":"json","column":220,"display":"missing field `submitted_hash` at line 1 column 220","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":220,"display":"missing field `submitted_hash` at line 1 column 220","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/null","direct":{"category":"Data","code":"json","column":191,"display":"invalid type: null, expected a string at line 1 column 191","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":191,"display":"invalid type: null, expected a string at line 1 column 191","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":null,\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/bool","direct":{"category":"Data","code":"json","column":191,"display":"invalid type: boolean `true`, expected a string at line 1 column 191","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":191,"display":"invalid type: boolean `true`, expected a string at line 1 column 191","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":true,\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/array","direct":{"category":"Data","code":"json","column":187,"display":"invalid type: sequence, expected a string at line 1 column 187","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":187,"display":"invalid type: sequence, expected a string at line 1 column 187","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":[],\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/object","direct":{"category":"Data","code":"json","column":187,"display":"invalid type: map, expected a string at line 1 column 187","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":187,"display":"invalid type: map, expected a string at line 1 column 187","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":{},\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/negative","direct":{"category":"Data","code":"json","column":189,"display":"invalid type: integer `-1`, expected a string at line 1 column 189","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":189,"display":"invalid type: integer `-1`, expected a string at line 1 column 189","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":-1,\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/float","direct":{"category":"Data","code":"json","column":190,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 190","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":190,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 190","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":1.5,\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/string","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"x","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"x","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"x\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_hash/u64-max","direct":{"category":"Data","code":"json","column":207,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 207","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":207,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 207","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":18446744073709551615,\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"missing/submitted_size","direct":{"category":"Data","code":"json","column":221,"display":"missing field `submitted_size` at line 1 column 221","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":221,"display":"missing field `submitted_size` at line 1 column 221","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/null","direct":{"category":"Data","code":"json","column":211,"display":"invalid type: null, expected u64 at line 1 column 211","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":211,"display":"invalid type: null, expected u64 at line 1 column 211","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":null,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/bool","direct":{"category":"Data","code":"json","column":211,"display":"invalid type: boolean `true`, expected u64 at line 1 column 211","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":211,"display":"invalid type: boolean `true`, expected u64 at line 1 column 211","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":true,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/array","direct":{"category":"Data","code":"json","column":207,"display":"invalid type: sequence, expected u64 at line 1 column 207","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":207,"display":"invalid type: sequence, expected u64 at line 1 column 207","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":[],\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/object","direct":{"category":"Data","code":"json","column":207,"display":"invalid type: map, expected u64 at line 1 column 207","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":207,"display":"invalid type: map, expected u64 at line 1 column 207","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":{},\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/negative","direct":{"category":"Data","code":"json","column":209,"display":"invalid value: integer `-1`, expected u64 at line 1 column 209","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":209,"display":"invalid value: integer `-1`, expected u64 at line 1 column 209","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":-1,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/float","direct":{"category":"Data","code":"json","column":210,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 210","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":210,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 210","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":1.5,\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/string","direct":{"category":"Data","code":"json","column":210,"display":"invalid type: string \"x\", expected u64 at line 1 column 210","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":210,"display":"invalid type: string \"x\", expected u64 at line 1 column 210","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":\"x\",\"transformation\":\"identity-v1\"}"},
{"case":"type/submitted_size/u64-max","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":18446744073709551615,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":18446744073709551615,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":18446744073709551615,\"transformation\":\"identity-v1\"}"},
{"case":"missing/transformation","direct":{"category":"Data","code":"json","column":209,"display":"missing field `transformation` at line 1 column 209","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":209,"display":"missing field `transformation` at line 1 column 209","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9}"},
{"case":"type/transformation/null","direct":{"category":"Data","code":"json","column":230,"display":"invalid type: null, expected a string at line 1 column 230","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":230,"display":"invalid type: null, expected a string at line 1 column 230","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":null}"},
{"case":"type/transformation/bool","direct":{"category":"Data","code":"json","column":230,"display":"invalid type: boolean `true`, expected a string at line 1 column 230","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":230,"display":"invalid type: boolean `true`, expected a string at line 1 column 230","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":true}"},
{"case":"type/transformation/array","direct":{"category":"Data","code":"json","column":226,"display":"invalid type: sequence, expected a string at line 1 column 226","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":226,"display":"invalid type: sequence, expected a string at line 1 column 226","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":[]}"},
{"case":"type/transformation/object","direct":{"category":"Data","code":"json","column":226,"display":"invalid type: map, expected a string at line 1 column 226","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":226,"display":"invalid type: map, expected a string at line 1 column 226","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":{}}"},
{"case":"type/transformation/negative","direct":{"category":"Data","code":"json","column":228,"display":"invalid type: integer `-1`, expected a string at line 1 column 228","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":228,"display":"invalid type: integer `-1`, expected a string at line 1 column 228","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":-1}"},
{"case":"type/transformation/float","direct":{"category":"Data","code":"json","column":229,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 229","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":229,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 229","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":1.5}"},
{"case":"type/transformation/string","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"x"}},"host":{"code":"invalid_operation","detail":"unsupported or inconsistent submitted input evidence","display":"invalid operation: unsupported or inconsistent submitted input evidence","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"x\"}"},
{"case":"type/transformation/u64-max","direct":{"category":"Data","code":"json","column":246,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 246","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":246,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 246","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":18446744073709551615}"},
{"case":"missing/state","direct":{"category":"Data","code":"json","column":219,"display":"missing field `state` at line 1 column 219","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":219,"display":"missing field `state` at line 1 column 219","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/null","direct":{"category":"Data","code":"json","column":161,"display":"invalid type: null, expected a string at line 1 column 161","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":161,"display":"invalid type: null, expected a string at line 1 column 161","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":null,\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/bool","direct":{"category":"Data","code":"json","column":161,"display":"invalid type: boolean `true`, expected a string at line 1 column 161","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":161,"display":"invalid type: boolean `true`, expected a string at line 1 column 161","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":true,\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/array","direct":{"category":"Data","code":"json","column":157,"display":"invalid type: sequence, expected a string at line 1 column 157","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":157,"display":"invalid type: sequence, expected a string at line 1 column 157","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":[],\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/object","direct":{"category":"Data","code":"json","column":157,"display":"invalid type: map, expected a string at line 1 column 157","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":157,"display":"invalid type: map, expected a string at line 1 column 157","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":{},\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/negative","direct":{"category":"Data","code":"json","column":159,"display":"invalid type: integer `-1`, expected a string at line 1 column 159","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":159,"display":"invalid type: integer `-1`, expected a string at line 1 column 159","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":-1,\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/float","direct":{"category":"Data","code":"json","column":160,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 160","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":160,"display":"invalid type: floating point `1.5`, expected a string at line 1 column 160","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":1.5,\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/string","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"x","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"code":"invalid_operation","detail":"unsupported input evidence record","display":"invalid operation: unsupported input evidence record","variant":"Invalid"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"x\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"type/state/u64-max","direct":{"category":"Data","code":"json","column":177,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 177","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":177,"display":"invalid type: integer `18446744073709551615`, expected a string at line 1 column 177","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":18446744073709551615,\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/null","direct":{"category":"Data","code":"json","column":149,"display":"invalid type: null, expected u64 at line 1 column 149","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":149,"display":"invalid type: null, expected u64 at line 1 column 149","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":null}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/negative","direct":{"category":"Data","code":"json","column":147,"display":"invalid value: integer `-1`, expected u64 at line 1 column 147","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":147,"display":"invalid value: integer `-1`, expected u64 at line 1 column 147","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":-1}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/bool","direct":{"category":"Data","code":"json","column":149,"display":"invalid type: boolean `true`, expected u64 at line 1 column 149","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":149,"display":"invalid type: boolean `true`, expected u64 at line 1 column 149","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":true}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/array","direct":{"category":"Data","code":"json","column":145,"display":"invalid type: sequence, expected u64 at line 1 column 145","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":145,"display":"invalid type: sequence, expected u64 at line 1 column 145","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":[]}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/object","direct":{"category":"Data","code":"json","column":145,"display":"invalid type: map, expected u64 at line 1 column 145","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":145,"display":"invalid type: map, expected u64 at line 1 column 145","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":{}}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/float","direct":{"category":"Data","code":"json","column":148,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 148","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":148,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 148","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":1.5}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/string","direct":{"category":"Data","code":"json","column":148,"display":"invalid type: string \"x\", expected u64 at line 1 column 148","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":148,"display":"invalid type: string \"x\", expected u64 at line 1 column 148","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":\"x\"}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/overflow-u32","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":4294967296}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":4294967296}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":4294967296}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/u64-max","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":18446744073709551615}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":18446744073709551615}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":18446744073709551615}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/size/absent","direct":{"category":"Data","code":"json","column":138,"display":"missing field `size` at line 1 column 138","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":138,"display":"missing field `size` at line 1 column 138","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/null","direct":{"category":"Data","code":"json","column":140,"display":"invalid type: null, expected u64 at line 1 column 140","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":140,"display":"invalid type: null, expected u64 at line 1 column 140","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":null,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/negative","direct":{"category":"Data","code":"json","column":138,"display":"invalid value: integer `-1`, expected u64 at line 1 column 138","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":138,"display":"invalid value: integer `-1`, expected u64 at line 1 column 138","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":-1,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/bool","direct":{"category":"Data","code":"json","column":140,"display":"invalid type: boolean `true`, expected u64 at line 1 column 140","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":140,"display":"invalid type: boolean `true`, expected u64 at line 1 column 140","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":true,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/array","direct":{"category":"Data","code":"json","column":136,"display":"invalid type: sequence, expected u64 at line 1 column 136","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":136,"display":"invalid type: sequence, expected u64 at line 1 column 136","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":[],\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/object","direct":{"category":"Data","code":"json","column":136,"display":"invalid type: map, expected u64 at line 1 column 136","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":136,"display":"invalid type: map, expected u64 at line 1 column 136","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":{},\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/float","direct":{"category":"Data","code":"json","column":139,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 139","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":139,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 139","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":1.5,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/string","direct":{"category":"Data","code":"json","column":139,"display":"invalid type: string \"x\", expected u64 at line 1 column 139","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":139,"display":"invalid type: string \"x\", expected u64 at line 1 column 139","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":\"x\",\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/overflow-u32","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":4294967296,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":4294967296,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":4294967296,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/u64-max","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":18446744073709551615,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":18446744073709551615,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":18446744073709551615,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_seconds/absent","direct":{"category":"Data","code":"json","column":126,"display":"missing field `modified_seconds` at line 1 column 126","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":126,"display":"missing field `modified_seconds` at line 1 column 126","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/null","direct":{"category":"Data","code":"json","column":110,"display":"invalid type: null, expected u32 at line 1 column 110","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":110,"display":"invalid type: null, expected u32 at line 1 column 110","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":null,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/negative","direct":{"category":"Data","code":"json","column":108,"display":"invalid value: integer `-1`, expected u32 at line 1 column 108","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":108,"display":"invalid value: integer `-1`, expected u32 at line 1 column 108","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":-1,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/bool","direct":{"category":"Data","code":"json","column":110,"display":"invalid type: boolean `true`, expected u32 at line 1 column 110","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":110,"display":"invalid type: boolean `true`, expected u32 at line 1 column 110","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":true,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/array","direct":{"category":"Data","code":"json","column":106,"display":"invalid type: sequence, expected u32 at line 1 column 106","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":106,"display":"invalid type: sequence, expected u32 at line 1 column 106","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":[],\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/object","direct":{"category":"Data","code":"json","column":106,"display":"invalid type: map, expected u32 at line 1 column 106","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":106,"display":"invalid type: map, expected u32 at line 1 column 106","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":{},\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/float","direct":{"category":"Data","code":"json","column":109,"display":"invalid type: floating point `1.5`, expected u32 at line 1 column 109","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":109,"display":"invalid type: floating point `1.5`, expected u32 at line 1 column 109","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":1.5,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/string","direct":{"category":"Data","code":"json","column":109,"display":"invalid type: string \"x\", expected u32 at line 1 column 109","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":109,"display":"invalid type: string \"x\", expected u32 at line 1 column 109","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":\"x\",\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/overflow-u32","direct":{"category":"Data","code":"json","column":116,"display":"invalid value: integer `4294967296`, expected u32 at line 1 column 116","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":116,"display":"invalid value: integer `4294967296`, expected u32 at line 1 column 116","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967296,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/u64-max","direct":{"category":"Data","code":"json","column":126,"display":"invalid value: integer `18446744073709551615`, expected u32 at line 1 column 126","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":126,"display":"invalid value: integer `18446744073709551615`, expected u32 at line 1 column 126","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":18446744073709551615,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/modified_nanos/absent","direct":{"category":"Data","code":"json","column":119,"display":"missing field `modified_nanos` at line 1 column 119","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":119,"display":"missing field `modified_nanos` at line 1 column 119","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/null","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":null,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/negative","direct":{"category":"Data","code":"json","column":100,"display":"invalid value: integer `-1`, expected u64 at line 1 column 100","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":100,"display":"invalid value: integer `-1`, expected u64 at line 1 column 100","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":-1,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/bool","direct":{"category":"Data","code":"json","column":102,"display":"invalid type: boolean `true`, expected u64 at line 1 column 102","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":102,"display":"invalid type: boolean `true`, expected u64 at line 1 column 102","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":true,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/array","direct":{"category":"Data","code":"json","column":98,"display":"invalid type: sequence, expected u64 at line 1 column 98","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":98,"display":"invalid type: sequence, expected u64 at line 1 column 98","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":[],\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/object","direct":{"category":"Data","code":"json","column":98,"display":"invalid type: map, expected u64 at line 1 column 98","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":98,"display":"invalid type: map, expected u64 at line 1 column 98","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":{},\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/float","direct":{"category":"Data","code":"json","column":101,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 101","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":101,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 101","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":1.5,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/string","direct":{"category":"Data","code":"json","column":101,"display":"invalid type: string \"x\", expected u64 at line 1 column 101","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":101,"display":"invalid type: string \"x\", expected u64 at line 1 column 101","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":\"x\",\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/overflow-u32","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"device":4294967296,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"device":4294967296,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":4294967296,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/u64-max","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"device":18446744073709551615,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"device":18446744073709551615,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"device\":18446744073709551615,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/device/absent","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/null","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":null,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/negative","direct":{"category":"Data","code":"json","column":99,"display":"invalid value: integer `-1`, expected u64 at line 1 column 99","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":99,"display":"invalid value: integer `-1`, expected u64 at line 1 column 99","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":-1,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/bool","direct":{"category":"Data","code":"json","column":101,"display":"invalid type: boolean `true`, expected u64 at line 1 column 101","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":101,"display":"invalid type: boolean `true`, expected u64 at line 1 column 101","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":true,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/array","direct":{"category":"Data","code":"json","column":97,"display":"invalid type: sequence, expected u64 at line 1 column 97","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":97,"display":"invalid type: sequence, expected u64 at line 1 column 97","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":[],\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/object","direct":{"category":"Data","code":"json","column":97,"display":"invalid type: map, expected u64 at line 1 column 97","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":97,"display":"invalid type: map, expected u64 at line 1 column 97","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":{},\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/float","direct":{"category":"Data","code":"json","column":100,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 100","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":100,"display":"invalid type: floating point `1.5`, expected u64 at line 1 column 100","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":1.5,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/string","direct":{"category":"Data","code":"json","column":100,"display":"invalid type: string \"x\", expected u64 at line 1 column 100","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":100,"display":"invalid type: string \"x\", expected u64 at line 1 column 100","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":\"x\",\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/overflow-u32","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"inode":4294967296,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"inode":4294967296,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":4294967296,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/u64-max","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"inode":18446744073709551615,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"inode":18446744073709551615,"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"inode\":18446744073709551615,\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"stamp/inode/absent","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/path/absent","direct":{"category":"Data","code":"json","column":118,"display":"missing field `path` at line 1 column 118","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":118,"display":"missing field `path` at line 1 column 118","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/path/null","direct":{"category":"Data","code":"json","column":61,"display":"invalid type: null, expected a string at line 1 column 61","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":61,"display":"invalid type: null, expected a string at line 1 column 61","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":null,\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/stamp/absent","direct":{"category":"Data","code":"json","column":80,"display":"missing field `stamp` at line 1 column 80","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":80,"display":"missing field `stamp` at line 1 column 80","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\"},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/stamp/null","direct":{"category":"Data","code":"json","column":92,"display":"invalid type: null, expected struct FileStamp at line 1 column 92","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":92,"display":"invalid type: null, expected struct FileStamp at line 1 column 92","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":null},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/hash/absent","direct":{"category":"Data","code":"json","column":128,"display":"missing field `hash` at line 1 column 128","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":128,"display":"missing field `hash` at line 1 column 128","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"reference/hash/null","direct":{"category":"Data","code":"json","column":41,"display":"invalid type: null, expected a string at line 1 column 41","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":41,"display":"invalid type: null, expected a string at line 1 column 41","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":null,\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"unknown/nested","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","reference_extra":[null,"é"],"stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1","z_extra":{"nested":["雪",{"x":false}],"null":null}}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","reference_extra":[null,"é"],"stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1","z_extra":{"nested":["雪",{"x":false}],"null":null}}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"reference_extra\":[null,\"é\"],\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7,\"stamp_extra\":{\"lost\":true}}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\",\"z_extra\":{\"nested\":[\"雪\",{\"x\":false}],\"null\":null}}"},
{"case":"semantic/unchecked-empty","direct":{"accepted":{"schema_version":1,"source":{"hash":"","path":"","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":0,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"","path":"","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":0,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"\",\"path\":\"\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":0,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/schema_version","direct":{"category":"Data","code":"json","column":36,"display":"duplicate field `schema_version` at line 1 column 36","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":36,"display":"duplicate field `schema_version` at line 1 column 36","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/source","direct":{"category":"Data","code":"json","column":157,"display":"duplicate field `source` at line 1 column 157","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":157,"display":"duplicate field `source` at line 1 column 157","line":1,"variant":"Json"},"input":"{\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/submitted_hash","direct":{"category":"Data","code":"json","column":206,"display":"duplicate field `submitted_hash` at line 1 column 206","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":206,"display":"duplicate field `submitted_hash` at line 1 column 206","line":1,"variant":"Json"},"input":"{\"submitted_hash\":\"\",\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/submitted_size","direct":{"category":"Data","code":"json","column":225,"display":"duplicate field `submitted_size` at line 1 column 225","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":225,"display":"duplicate field `submitted_size` at line 1 column 225","line":1,"variant":"Json"},"input":"{\"submitted_size\":9,\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/transformation","direct":{"category":"Data","code":"json","column":256,"display":"duplicate field `transformation` at line 1 column 256","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":256,"display":"duplicate field `transformation` at line 1 column 256","line":1,"variant":"Json"},"input":"{\"transformation\":\"identity-v1\",\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/state","direct":{"category":"Data","code":"json","column":177,"display":"duplicate field `state` at line 1 column 177","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":177,"display":"duplicate field `state` at line 1 column 177","line":1,"variant":"Json"},"input":"{\"state\":\"referenced\",\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/unknown","direct":{"accepted":{"extra":{"last":null},"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"extra":{"last":null},"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"extra\":1,\"extra\":{\"last\":null},\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/reference-path","direct":{"category":"Data","code":"json","column":71,"display":"duplicate field `path` at line 1 column 71","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":71,"display":"duplicate field `path` at line 1 column 71","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"first\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/stamp-size","direct":{"category":"Data","code":"json","column":153,"display":"duplicate field `size` at line 1 column 153","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":153,"display":"duplicate field `size` at line 1 column 153","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":1,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/reference-extra","direct":{"accepted":{"schema_version":1,"source":{"extra":null,"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"extra":null,"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"extra\":1,\"extra\":null,\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"duplicate/stamp-unknown","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":4294967295,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"extra\":1,\"extra\":null,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"overflow/schema_version","direct":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"input":"{\"schema_version\":18446744073709551616,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"overflow/submitted_size","direct":{"category":"Data","code":"json","column":227,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u64 at line 1 column 227","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":227,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u64 at line 1 column 227","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":{\"modified_nanos\":4294967295,\"modified_seconds\":0,\"size\":7}},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":18446744073709551616,\"transformation\":\"identity-v1\"}"},
{"case":"sequence/reference","direct":{"category":"Data","code":"json","column":29,"display":"invalid type: sequence, expected struct InputReference at line 1 column 29","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":29,"display":"invalid type: sequence, expected struct InputReference at line 1 column 29","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":[\"x\",{\"modified_nanos\":0,\"modified_seconds\":0,\"size\":0},\"h\"],\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"sequence/stamp/168","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":0,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":0,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":[7,0,0]},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"sequence/stamp/169","direct":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":0,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"host":{"accepted":{"schema_version":1,"source":{"hash":"not-a-hash","path":"missing/原本.fasta","stamp":{"modified_nanos":0,"modified_seconds":0,"size":7}},"state":"referenced","submitted_hash":"","submitted_size":9,"transformation":"identity-v1"}},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":[7,0,0,null,null]},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"sequence/stamp/170","direct":{"category":"Syntax","code":"json","column":106,"display":"trailing characters at line 1 column 106","line":1,"variant":"Json"},"host":{"category":"Syntax","code":"json","column":106,"display":"trailing characters at line 1 column 106","line":1,"variant":"Json"},"input":"{\"schema_version\":1,\"source\":{\"hash\":\"not-a-hash\",\"path\":\"missing/原本.fasta\",\"stamp\":[7,0,0,null,null,1]},\"state\":\"referenced\",\"submitted_hash\":\"\",\"submitted_size\":9,\"transformation\":\"identity-v1\"}"},
{"case":"syntax/171","direct":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"input":"[]"},
{"case":"syntax/172","direct":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"input":"[1,null,\"h\",0,\"identity-v1\",\"generated\"]"},
{"case":"syntax/173","direct":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":0,"display":"invalid type: sequence, expected struct SubmittedInput at line 1 column 0","line":1,"variant":"Json"},"input":"[1,null,\"h\",0,\"identity-v1\",\"generated\",{}]"},
{"case":"syntax/174","direct":{"category":"Data","code":"json","column":4,"display":"invalid type: null, expected struct SubmittedInput at line 1 column 4","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":4,"display":"invalid type: null, expected struct SubmittedInput at line 1 column 4","line":1,"variant":"Json"},"input":"null"},
{"case":"syntax/175","direct":{"category":"Data","code":"json","column":4,"display":"invalid type: boolean `true`, expected struct SubmittedInput at line 1 column 4","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":4,"display":"invalid type: boolean `true`, expected struct SubmittedInput at line 1 column 4","line":1,"variant":"Json"},"input":"true"},
{"case":"syntax/176","direct":{"category":"Data","code":"json","column":1,"display":"invalid type: integer `1`, expected struct SubmittedInput at line 1 column 1","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":1,"display":"invalid type: integer `1`, expected struct SubmittedInput at line 1 column 1","line":1,"variant":"Json"},"input":"1"},
{"case":"syntax/177","direct":{"category":"Data","code":"json","column":2,"display":"missing field `schema_version` at line 1 column 2","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":2,"display":"missing field `schema_version` at line 1 column 2","line":1,"variant":"Json"},"input":"{}"},
{"case":"syntax/178","direct":{"category":"Eof","code":"json","column":1,"display":"EOF while parsing an object at line 1 column 1","line":1,"variant":"Json"},"host":{"category":"Eof","code":"json","column":1,"display":"EOF while parsing an object at line 1 column 1","line":1,"variant":"Json"},"input":"{"},
{"case":"syntax/179","direct":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u32 at line 1 column 38","line":1,"variant":"Json"},"input":"{\"schema_version\":18446744073709551616}"},
{"case":"syntax/180","direct":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u64 at line 1 column 38","line":1,"variant":"Json"},"host":{"category":"Data","code":"json","column":38,"display":"invalid type: floating point `1.8446744073709552e+19`, expected u64 at line 1 column 38","line":1,"variant":"Json"},"input":"{\"submitted_size\":18446744073709551616}"}
]"###;
#[test]
fn original_direct_serde_and_host_reader_matrix() {
    let Some(root) = isolated_profile::enter("original_direct_serde_and_host_reader_matrix") else {
        return;
    };
    let mut observations = vec![];
    for (name, input) in reader_cases() {
        fs::write(root.join("input.json"), input.as_bytes()).unwrap();
        let direct = match serde_json::from_str::<OldSubmittedInput>(&input) {
            Ok(v) => json!({"accepted":serde_json::to_value(v).unwrap()}),
            Err(e) => json_error(&e),
        };
        let host = match evidence::read_input(&root) {
            Ok(v) => json!({"accepted":serde_json::to_value(v).unwrap()}),
            Err(e) => error(&e),
        };
        observations.push(json!({"case":name,"input":input,"direct":direct,"host":host}));
    }
    retain("reader-observations.json", &bytes(&observations));
    assert_eq!(
        json!(observations),
        serde_json::from_str::<Value>(READER_OBSERVATIONS).unwrap()
    );
}

#[test]
fn injected_extensions_survive_real_rewrite_except_unknown_stamp_fields() {
    let Some(root) = isolated_profile::enter(
        "injected_extensions_survive_real_rewrite_except_unknown_stamp_fields",
    ) else {
        return;
    };
    fs::write(root.join("source.fasta"), SOURCE).unwrap();
    let (run, mut old) = reference(&root, &[]);
    old.extra.insert(
        "top_extra".into(),
        json!({"null":null,"escaped":"\"\n雪","nested":[null,{"z":2,"a":1}]}),
    );
    old.source.as_mut().unwrap().extra.insert(
        "reference_extra".into(),
        json!({"unknown":[false,null,"é"]}),
    );
    // Explicitly injected input, never labelled a fresh producer capture.
    let mut injected = serde_json::to_value(&old).unwrap();
    injected["source"]["stamp"]["stamp_extra"] = json!({"historically_lost":true});
    evidence::write_json(&run.directory().join("input.json"), &injected).unwrap();
    retain(
        "injected.before.json",
        &fs::read(run.directory().join("input.json")).unwrap(),
    );
    evidence::preserve_input(run.directory()).unwrap();
    old.state = "preserved".into();
    old.extra
        .insert("preserved_original_bytes".into(), json!(SOURCE.len()));
    capture("injected-preserved", run.directory(), &old);
    assert_eq!(
        fs::read(run.directory().join("preserved-input")).unwrap(),
        SOURCE
    );
    let moved = root.join("copy.fasta");
    fs::write(&moved, SOURCE).unwrap();
    evidence::relink(run.directory(), &moved).unwrap();
    old.state = "referenced".into();
    old.source.as_mut().unwrap().path = "../../../../copy.fasta".into();
    old.source.as_mut().unwrap().stamp = native_stamp(&moved);
    capture("injected-relinked", run.directory(), &old);
    // Flatten collisions are emitted, then refused as duplicate known fields.
    old.extra.insert("state".into(), json!("future"));
    evidence::write_json(&run.directory().join("input.json"), &old).unwrap();
    assert_eq!(
        fs::read(run.directory().join("input.json")).unwrap(),
        bytes(&old)
    );
    assert!(matches!(
        evidence::read_input(run.directory()),
        Err(Error::Json(_))
    ));
}

#[test]
fn reached_read_copy_and_source_change_faults() {
    let Some(root) = isolated_profile::enter("reached_read_copy_and_source_change_faults") else {
        return;
    };
    let e = evidence::read_input(&root).unwrap_err();
    assert!(matches!(&e,Error::Io(io) if io.kind()==std::io::ErrorKind::NotFound));
    assert_eq!(e.code(), "io");
    #[cfg(target_os = "linux")]
    assert_eq!(e.to_string(), "No such file or directory (os error 2)");
    retain("missing-record.error.json", &bytes(&error(&e)));
    fs::write(root.join("input.json"), b"{").unwrap();
    let e = evidence::read_input(&root).unwrap_err();
    assert_eq!(
        error(&e),
        json!({"variant":"Json","code":"json","display":"EOF while parsing an object at line 1 column 1","line":1,"column":1,"category":"Eof"})
    );
    let sparse = fs::File::create(root.join("input.json")).unwrap();
    sparse.set_len((64 << 20) + 1).unwrap();
    drop(sparse);
    let e = evidence::read_input(&root).unwrap_err();
    assert_eq!(
        e.to_string(),
        "invalid operation: provenance record exceeds reading limit (at most 64 MiB)"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(root.join("input.json")).unwrap();
        fs::write(root.join("real.json"), b"{").unwrap();
        symlink(root.join("real.json"), root.join("input.json")).unwrap();
        let e = evidence::read_input(&root).unwrap_err();
        assert_eq!(
            e.to_string(),
            "invalid operation: unsafe input evidence record"
        );
        assert_eq!(e.code(), "invalid_operation");
        retain("symlink.error.json", &bytes(&error(&e)));
    }
    fs::write(root.join("source.fasta"), SOURCE).unwrap();
    let (run, old) = reference(&root, &[]);
    let before = bytes(&old);
    fs::write(root.join("source.fasta"), b">sample\nTCGA\n").unwrap();
    let e = evidence::preserve_input(run.directory()).unwrap_err();
    assert_eq!(
        e.to_string(),
        "invalid operation: source changed: copied bytes are not the analysed original; overwrite cancelled"
    );
    retain("source-changed.error.json", &bytes(&error(&e)));
    assert_eq!(
        fs::read(run.directory().join("input.json")).unwrap(),
        before
    );
    assert!(!run.directory().join("preserved-input").exists());
    fs::remove_file(root.join("source.fasta")).unwrap();
    let e = evidence::preserve_input(run.directory()).unwrap_err();
    assert!(matches!(&e,Error::Io(io) if io.kind()==std::io::ErrorKind::NotFound));
    retain("copy-open.error.json", &bytes(&error(&e)));
    assert_eq!(
        fs::read(run.directory().join("input.json")).unwrap(),
        before
    );
    assert!(!run.directory().join("preserved-input").exists());
    #[cfg(target_os = "linux")]
    {
        // Linux opens directories for reading; std::io::copy then fails on read.
        // This is a reached copy failure, separately from the missing open above.
        fs::create_dir(root.join("source.fasta")).unwrap();
        let e = evidence::preserve_input(run.directory()).unwrap_err();
        assert!(matches!(&e, Error::Io(io) if io.kind() == std::io::ErrorKind::IsADirectory));
        assert_eq!(e.code(), "io");
        assert_eq!(e.to_string(), "Is a directory (os error 21)");
        retain("copy-read.error.json", &bytes(&error(&e)));
        assert_eq!(
            fs::read(run.directory().join("input.json")).unwrap(),
            before
        );
        assert!(!run.directory().join("preserved-input").exists());
        fs::remove_dir(root.join("source.fasta")).unwrap();
    }
    // A real generated writer accepts arbitrary transformation; reader then refuses.
    let generated =
        ArtifactRun::create(&root.join("result.txt"), "reader-refused-generated").unwrap();
    evidence::link_generated_input(&generated, &[], PAYLOAD, "future-transform", "Synthetic")
        .unwrap();
    let expected = OldSubmittedInput {
        schema_version: 1,
        source: None,
        submitted_hash: basecraft_io::hash_bytes(PAYLOAD),
        submitted_size: PAYLOAD.len() as u64,
        transformation: "future-transform".into(),
        state: "generated".into(),
        extra: BTreeMap::from([("upstream".into(), json!([]))]),
    };
    capture("reader-refused-generated", generated.directory(), &expected);
    assert_eq!(
        evidence::read_input(generated.directory())
            .unwrap_err()
            .to_string(),
        "invalid operation: unsupported or inconsistent submitted input evidence"
    );
}

#[cfg(unix)]
#[test]
fn reached_atomic_ledger_write_obstruction_retains_original_and_copy() {
    use std::os::unix::fs::PermissionsExt;
    let Some(root) = isolated_profile::enter(
        "reached_atomic_ledger_write_obstruction_retains_original_and_copy",
    ) else {
        return;
    };
    fs::write(root.join("source.fasta"), SOURCE).unwrap();
    let (run, old) = reference(&root, &[]);
    // Recovered partial artifact already exists, so resolve verifies it, skips
    // copy/temp creation, and reaches write_json -> atomic writer's temp open.
    fs::write(run.directory().join("preserved-input"), SOURCE).unwrap();
    fs::set_permissions(run.directory(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = evidence::preserve_input(run.directory());
    fs::set_permissions(run.directory(), fs::Permissions::from_mode(0o755)).unwrap();
    let e = result.unwrap_err();
    assert!(
        matches!(&e, Error::Format(basecraft_io::Error::Io(io)) if io.kind() == std::io::ErrorKind::PermissionDenied),
        "{e:?}"
    );
    assert_eq!(e.code(), "format");
    retain("atomic-ledger-write.error.json", &bytes(&error(&e)));
    assert_eq!(
        fs::read(run.directory().join("input.json")).unwrap(),
        bytes(&old)
    );
    assert_eq!(
        fs::read(run.directory().join("preserved-input")).unwrap(),
        SOURCE
    );
    assert_eq!(fs::read(root.join("source.fasta")).unwrap(), SOURCE);
    retain("atomic-ledger-write.original.json", &bytes(&old));
    retain("atomic-ledger-write.partial-preserved-input", SOURCE);
    evidence::preserve_input(run.directory()).unwrap();
    let mut retried = old;
    retried.state = "preserved".into();
    retried
        .extra
        .insert("preserved_original_bytes".into(), json!(0));
    capture("partial-retry", run.directory(), &retried);
}
