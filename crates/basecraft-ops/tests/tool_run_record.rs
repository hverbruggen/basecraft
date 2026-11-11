//! A tool run's bookkeeping travels in typed `tool.run` fields, survives the
//! journal and replays; nothing is hidden in parameter names.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
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
    assert!(records::verify(&target, None).unwrap().is_empty());
}
