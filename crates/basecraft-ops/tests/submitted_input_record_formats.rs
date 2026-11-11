//! Input-ledger byte compatibility and public preserve/relink behavior.
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
    assert!(matches!(e, Error::Limit { limit, .. } if limit == 64 << 20));
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
