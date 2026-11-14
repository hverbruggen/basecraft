//! A tool run's bookkeeping travels in typed `tool.run` fields, survives the
//! journal and replays; nothing is hidden in parameter names.
use basecraft_ops::{Session, log, records};
use basecraft_protocol::Op;

#[test]
fn support_import_records_typed_contributions_and_input_run() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.nwk");
    std::fs::write(&target, "((A:1,B:1)90:1,(C:1,D:1)80:1);").unwrap();
    let source = dir.path().join("support.nwk");
    std::fs::write(&source, "((A:1,B:1)75:1,(C:1,D:1)60:1);").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&target).unwrap();
    session.import_support(doc, &source, "Bootstrap").unwrap();
    session.save(doc, None).unwrap();

    let entries = log::read_all(dir.path()).unwrap();
    let runs: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry.op, Op::ToolRun { .. }))
        .collect();
    assert_eq!(runs.len(), 1);
    let Op::ToolRun {
        params,
        input_run,
        contributions,
        ..
    } = &runs[0].op
    else {
        unreachable!()
    };
    assert!(input_run.is_some());
    assert_eq!(contributions.as_ref().unwrap().edits.len(), 1);
    assert!(params.get("contributions").is_none() && params.get("input_run").is_none());
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    assert_eq!(
        records::verify(&target, None).unwrap(),
        Vec::<String>::new()
    );
}

/// A run with a folder names it in the entry; the entry survives the journal
/// and replays, and the parameters stay the tool's own.
#[test]
fn a_tool_run_names_its_run_folder_and_replays() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let output = dir.path().join("result.fasta");
    std::fs::write(&output, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    let inputs = vec![(
        "gene.fasta".to_string(),
        session.snapshot_file(&file).unwrap(),
    )];
    let run = records::ArtifactRun::create(None, &output, "01M79REPLAYRUNFOLDER00000").unwrap();
    session
        .record_tool_run(
            doc,
            basecraft_protocol::Actor::Plugin {
                id: "example".into(),
                version: "1".into(),
            },
            basecraft_ops::session::ToolRunRecord {
                run: Some(&run),
                ..basecraft_ops::session::ToolRunRecord::new(
                    ("example", "analyze"),
                    serde_json::json!({"model":"p"}),
                    &inputs,
                    &[(output.clone(), basecraft_io::hash_file(&output).unwrap())],
                )
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();

    let entries = log::read_all(dir.path()).unwrap();
    let link = entries
        .iter()
        .find_map(|entry| match &entry.op {
            Op::ToolRun { run, params, .. } => {
                assert_eq!(*params, serde_json::json!({"model":"p"}));
                run.clone()
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(link.id, "01M79REPLAYRUNFOLDER00000");
    assert_eq!(
        dir.path().join(&link.folder).canonicalize().unwrap(),
        run.directory().canonicalize().unwrap()
    );
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}
