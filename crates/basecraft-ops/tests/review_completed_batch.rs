//! Independent review regressions. These assert the safe
//! contract, adapted to confirmed selection and retained registered-run evidence.
#[path = "support/submission_run.rs"]
mod submission_run;
use std::path::{Path, PathBuf};

use basecraft_ops::{Session, metadata, submission};
use basecraft_platform::settings::Submitter;
use serde_json::{Value, json};

fn package(root: &Path, name: &str) -> PathBuf {
    let alignment = root.join(format!("{name}.afa"));
    std::fs::write(&alignment, ">a@b\nACGT\n>a_b\nACGT\n").unwrap();
    let mut session = Session::unlogged(root);
    let doc = submission::create(
        &mut session,
        Path::new(&format!("{name}.submission.json")),
        &alignment,
    )
    .unwrap();
    let package = root.join(name);
    let selection_hash = submission::review(&session, doc, &Submitter::default())
        .unwrap()
        .selection_hash
        .unwrap();
    submission::edit(
        &mut session,
        doc,
        basecraft_protocol::submission::SubmissionEdit::ConfirmSelection { selection_hash },
    )
    .unwrap();
    submission::export(&mut session, doc, &package, &Submitter::default()).unwrap();
    package
}

fn outcome(root: &Path, package: &Path, exit_code: i32) -> Value {
    let request = submission::validation_request(root, package).unwrap();
    let reports = root.join(request["destinations"]["reports"].as_str().unwrap());
    std::fs::create_dir_all(&reports).unwrap();
    for (name, text) in [
        ("submission.sqn", "old sequence payload"),
        ("submission.gbf", "old flatfile payload"),
        (
            "submission.val",
            "Warning: valid [TEST.Warning] review this\n",
        ),
        ("table2asn.log", "test stand-in reports\n"),
    ] {
        std::fs::write(reports.join(name), text).unwrap();
    }
    let mut outcome = json!({"state":"completed", "run_id":"review-run", "data":{
        "status":"completed", "version":"test", "exit_code":exit_code
    }, "artifacts":[{"role":"reports", "path":reports.join("table2asn.log")} ]});
    submission_run::retain(root, &request, &mut outcome);
    outcome
}

#[test]
fn review_nonzero_tool_exit_cannot_pass_with_warning_report() {
    let dir = tempfile::tempdir().unwrap();
    let package = package(dir.path(), "batch");
    let outcome = outcome(dir.path(), &package, 1);
    let summary = submission::record_validation(dir.path(), &package, &outcome).unwrap();
    assert_eq!(summary.status, "failed", "{summary:?}");
    assert_eq!(summary.warnings, 1, "retain parsed issues on failed exits");
    assert!(!package.join("genbank/submission.sqn").exists());
}

#[test]
fn review_old_outcome_cannot_validate_regenerated_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let package = package(root, "batch");
    let old_outcome = outcome(root, &package, 0);
    let mut session = Session::unlogged(root);
    let doc = session.open(&root.join("batch.submission.json")).unwrap();
    // Regenerate legitimately, with changed submitter bytes and therefore a new
    // package input hash, while the old validation job is finishing.
    let contact = Submitter {
        first_name: "Changed".into(),
        ..Submitter::default()
    };
    let changed = submission::export(&mut session, doc, &package, &contact).unwrap();
    let newer_outcome = outcome(root, &package, 0);
    let newer = submission::record_validation(root, &package, &newer_outcome).unwrap();
    assert_eq!(newer.status, "passed");
    match submission::record_validation(root, &package, &old_outcome) {
        Err(_) => (),
        Ok(summary) => assert_ne!(
            summary.status, "passed",
            "old reports accepted for {}: {summary:?}",
            changed.input_hash
        ),
    }
}

#[test]
fn review_accession_uses_export_identity_despite_original_name_collision() {
    let dir = tempfile::tempdir().unwrap();
    package(dir.path(), "batch");
    let mut session = Session::unlogged(dir.path());
    let doc = session
        .open(&dir.path().join("batch.submission.json"))
        .unwrap();
    let review = submission::review(&session, doc, &Submitter::default()).unwrap();
    assert_eq!(review.prepared.records[0].record, "a@b");
    assert_eq!(review.prepared.records[0].export_id.as_deref(), Some("a_b"));
    assert_eq!(
        review.prepared.records[1].export_id.as_deref(),
        Some("a_b_2")
    );
    let plan = submission::plan_accessions(&session, doc, "a_b\tPX123456\n", &Submitter::default())
        .unwrap();
    assert_eq!(
        plan.accessions[0].record, "a@b",
        "accession was assigned to the wrong biological record"
    );
}

#[test]
fn review_metadata_partial_write_reports_completed_table_replacements() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = metadata::create(root).unwrap();
    let mut session = Session::unlogged(root);
    let doc = session.open(&manifest).unwrap();
    let original = std::fs::read(&manifest).unwrap();
    let destination = root.join("copy");
    std::fs::create_dir(&destination).unwrap();
    // Reverse write order replaces records and identifications, then fails
    // replacing this directory. The manifest has not been written yet.
    std::fs::create_dir(destination.join("taxa.tsv")).unwrap();
    let error = session
        .save(doc, Some(&destination.join(metadata::MANIFEST)))
        .unwrap_err()
        .to_string();
    assert!(destination.join("records.tsv").is_file());
    assert!(destination.join("identifications.tsv").is_file());
    assert!(!destination.join(metadata::MANIFEST).exists());
    assert!(
        error.contains("partial") || error.contains("incomplete"),
        "partial metadata write was not explained: {error}"
    );
    assert!(error.contains("records.tsv") && error.contains("identifications.tsv"));
    assert!(error.contains("retry") && error.contains("taxa.tsv"));
    assert_eq!(std::fs::read(&manifest).unwrap(), original);
    std::fs::remove_dir(destination.join("taxa.tsv")).unwrap();
    session
        .save(doc, Some(&destination.join(metadata::MANIFEST)))
        .unwrap();
    metadata::load(&destination.join(metadata::MANIFEST)).unwrap();
    assert_eq!(std::fs::read(&manifest).unwrap(), original);
}

#[test]
fn unrelated_package_and_modified_artifacts_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let a = package(dir.path(), "a");
    let b = package(dir.path(), "b");
    let result = outcome(dir.path(), &a, 0);
    assert!(submission::record_validation(dir.path(), &b, &result).is_err());
    let artifact = result["artifacts"][0]["path"].as_str().unwrap();
    std::fs::write(artifact, "tampered").unwrap();
    assert!(submission::record_validation(dir.path(), &a, &result).is_err());
    assert!(!a.join("genbank/submission.sqn").exists());
}

#[test]
fn incomplete_and_unknown_reports_fail_but_quiet_complete_run_passes() {
    for (preview, report, expected) in [
        (
            false,
            Some("Warning: valid [TEST.Warning] review\n"),
            "failed",
        ),
        (
            true,
            Some("unrecognized validator report format\n"),
            "failed",
        ),
        (true, None, "passed"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let package = package(dir.path(), "batch");
        let request = submission::validation_request(dir.path(), &package).unwrap();
        let reports = dir
            .path()
            .join(request["destinations"]["reports"].as_str().unwrap());
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(reports.join("table2asn.log"), "log").unwrap();
        std::fs::write(reports.join("submission.sqn"), "sqn").unwrap();
        if preview {
            std::fs::write(reports.join("submission.gbf"), "gbf").unwrap();
        }
        if let Some(report) = report {
            std::fs::write(reports.join("submission.val"), report).unwrap();
        }
        let mut result = json!({"state":"completed","data":{"exit_code":0,"version":"test"}});
        submission_run::retain(dir.path(), &request, &mut result);
        let summary = submission::record_validation(dir.path(), &package, &result).unwrap();
        assert_eq!(summary.status, expected, "{summary:?}");
        assert_eq!(
            package.join("genbank/submission.sqn").exists(),
            expected == "passed"
        );
    }
}

#[test]
fn returned_ids_survive_selection_save_reopen_and_refuse_ambiguous_lines() {
    use basecraft_protocol::submission::{Decision, RowDecision, SubmissionEdit};
    let dir = tempfile::tempdir().unwrap();
    package(dir.path(), "batch");
    let mut session = Session::unlogged(dir.path());
    let doc = session
        .open(&dir.path().join("batch.submission.json"))
        .unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "a@b".into(),
                decision: Decision::Exclude,
            }],
        },
    )
    .unwrap();
    session.save(doc, None).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let doc = reopened
        .open(&dir.path().join("batch.submission.json"))
        .unwrap();
    let plan = submission::plan_accessions(
        &reopened,
        doc,
        "a_b\tPX123456\na_b a_b_2 PX123457\n",
        &Submitter::default(),
    )
    .unwrap();
    assert_eq!(plan.accessions.len(), 1);
    assert_eq!(plan.accessions[0].record, "a@b");
    assert_eq!(plan.unmatched.len(), 1);
}

#[test]
fn reexported_seqid_with_two_record_meanings_is_ambiguous() {
    use basecraft_protocol::submission::{Decision, RowDecision, SubmissionEdit};
    let dir = tempfile::tempdir().unwrap();
    let folder = package(dir.path(), "batch");
    let mut session = Session::unlogged(dir.path());
    let doc = session
        .open(&dir.path().join("batch.submission.json"))
        .unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "a@b".into(),
                decision: Decision::Exclude,
            }],
        },
    )
    .unwrap();
    let selection_hash = submission::review(&session, doc, &Submitter::default())
        .unwrap()
        .selection_hash
        .unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::ConfirmSelection { selection_hash },
    )
    .unwrap();
    submission::export(&mut session, doc, &folder, &Submitter::default()).unwrap();
    let plan =
        submission::plan_accessions(&session, doc, "a_b PX123456", &Submitter::default()).unwrap();
    assert_eq!(plan.accessions, Vec::new());
    assert_eq!(plan.unmatched, ["a_b PX123456"]);
}

#[test]
fn accession_shaped_submitted_id_is_identity_and_multiple_returns_are_ambiguous() {
    use basecraft_protocol::submission::{Decision, RowDecision, SubmissionEdit};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("batch.afa"), ">AB123456\nACGT\n>a@b\nACGT\n").unwrap();
    let mut session = Session::unlogged(root);
    let doc = submission::create(
        &mut session,
        Path::new("batch.submission.json"),
        Path::new("batch.afa"),
    )
    .unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "AB123456".into(),
                decision: Decision::New,
            }],
        },
    )
    .unwrap();
    let selection_hash = submission::review(&session, doc, &Submitter::default())
        .unwrap()
        .selection_hash
        .unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::ConfirmSelection { selection_hash },
    )
    .unwrap();
    submission::export(
        &mut session,
        doc,
        Path::new("package"),
        &Submitter::default(),
    )
    .unwrap();
    let mut reopened = Session::unlogged(root);
    let doc = reopened.open(&root.join("batch.submission.json")).unwrap();
    for text in [
        "AB123456\tPX123456",
        "AB123456,PX123456",
        "PX123456 AB123456",
        "a_b PX123456",
        "a@b PX123456",
    ] {
        let plan =
            submission::plan_accessions(&reopened, doc, text, &Submitter::default()).unwrap();
        assert_eq!(plan.accessions.len(), 1, "{text}: {plan:?}");
        assert_eq!(plan.accessions[0].accession, "PX123456", "{text}");
        assert_eq!(
            plan.accessions[0].record,
            if text.contains("AB123456") {
                "AB123456"
            } else {
                "a@b"
            }
        );
        assert_eq!(plan.unmatched, Vec::<String>::new());
    }
    for text in [
        "AB123456",
        "AB123456 PX123456 PX123457",
        "AB123456 PX123456 PX123456",
        "a_b PX123456 PX123457",
        "AB123456 a_b PX123456",
        "AB123456 AB123456 PX123456",
    ] {
        let plan =
            submission::plan_accessions(&reopened, doc, text, &Submitter::default()).unwrap();
        assert_eq!(plan.accessions, Vec::new(), "{text}");
        assert_eq!(plan.unmatched, [text], "{text}");
    }
}
