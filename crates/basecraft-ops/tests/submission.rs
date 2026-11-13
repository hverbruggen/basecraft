//! GenBank marker submissions: the document, review, package, validation
//! records, accessions, undo and replay.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
#[path = "support/submission_run.rs"]
mod submission_run;
use std::path::{Path, PathBuf};

use basecraft_docs::submission::Severity;
use basecraft_ops::metadata::{self, ImportOptions, SheetImport};
use basecraft_ops::submission::{self, Review};
use basecraft_ops::{DocId, Session, records};
use basecraft_platform::settings::Submitter;
use basecraft_protocol::submission::{
    Author, BatchSettings, Decision, EndPolicy, FrameChoice, ItsComponent, Marker, MolType,
    Organelle, Publication, RowDecision, SubmissionEdit,
};
use serde_json::json;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/submission")
}

/// A project with the fixture alignments and the specimen sheet imported.
fn project(root: &Path) {
    for name in ["rbcL.afa", "COI.afa", "18S.afa", "ITS.afa", "specimens.csv"] {
        std::fs::copy(fixtures().join(name), root.join(name)).unwrap();
    }
    let mut s = Session::new(root);
    let doc = metadata::open(&mut s, true).unwrap().unwrap();
    let plan = metadata::plan_sheet_import(
        &s,
        doc,
        &SheetImport {
            path: "specimens.csv".into(),
            mapping: None,
            document: None,
            options: ImportOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(plan.changes.links.len(), 13, "{:#?}", plan.issues);
    metadata::apply_plan(&mut s, doc, plan.changes).unwrap();
    s.save(doc, None).unwrap();
}

fn submitter() -> Submitter {
    Submitter {
        first_name: "Jane".into(),
        last_name: "Doe".into(),
        email: "jane@example.org".into(),
        institution: "University of Melbourne".into(),
        department: "School of BioSciences".into(),
        city: "Melbourne".into(),
        state: "Victoria".into(),
        postal_code: "3010".into(),
        country: "Australia".into(),
        ..Submitter::default()
    }
}

fn publication() -> SubmissionEdit {
    SubmissionEdit::SetPublication {
        publication: Publication {
            title: "Marker sequences for submission tests".into(),
            authors: vec![Author {
                first: "Jane".into(),
                last: "Doe".into(),
                middle: String::new(),
            }],
        },
    }
}

fn rbcl_batch() -> SubmissionEdit {
    SubmissionEdit::SetBatch {
        batch: BatchSettings {
            marker: Marker::Coding {
                gene: "rbcL".into(),
                product: "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit".into(),
            },
            mol_type: MolType::GenomicDna,
            organelle: Some(Organelle::Chloroplast),
            genetic_code: Some(11),
            ends: EndPolicy::Auto,
        },
    }
}

fn codes(review: &Review, record: &str) -> Vec<String> {
    review
        .prepared
        .issues
        .iter()
        .filter(|i| i.record.as_deref() == Some(record))
        .map(|i| i.code.clone())
        .collect()
}

fn confirm_selection(s: &mut Session, doc: DocId) {
    let selection_hash = submission::review(s, doc, &submitter())
        .unwrap()
        .selection_hash
        .unwrap();
    submission::edit(s, doc, SubmissionEdit::ConfirmSelection { selection_hash }).unwrap();
}

fn rbcl(s: &mut Session) -> DocId {
    let doc = submission::create(
        s,
        Path::new("submissions/rbcL.submission.json"),
        Path::new("rbcL.afa"),
    )
    .unwrap();
    for edit in [
        rbcl_batch(),
        publication(),
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "REF_rbcL".into(),
                decision: Decision::Existing {
                    accession: "AB123456.1".into(),
                },
            }],
        },
    ] {
        submission::edit(s, doc, edit).unwrap();
    }
    submission::edit(
        s,
        doc,
        SubmissionEdit::SetRows {
            rows: ["HV101_rbcL", "HV102_rbcL", "HV103_rbcL"]
                .iter()
                .map(|r| RowDecision {
                    record: (*r).into(),
                    decision: Decision::New,
                })
                .collect(),
        },
    )
    .unwrap();
    confirm_selection(s, doc);
    doc
}

#[test]
fn coding_review_frames_undo_save_reload_and_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    {
        let mut s = Session::new(root);
        let doc = rbcl(&mut s);
        let review = submission::review(&s, doc, &submitter()).unwrap();
        let p = &review.prepared;
        let hv101 = &p.records[0];
        assert_eq!(hv101.frame.as_ref().unwrap().detected, Some(2));
        assert!(codes(&review, "HV101_rbcL").contains(&"frame_unconfirmed".into()));
        assert!(codes(&review, "HV101_rbcL").contains(&"masked_columns".into()));
        let hv102 = &p.records[1];
        assert_eq!(hv102.trimmed, (2, 0));
        // Trimming two Ns moves the frame: codon_start 3 in the export.
        assert_eq!(hv102.frame.as_ref().unwrap().detected, Some(3));
        assert!(hv102.residues.contains('R') && hv102.residues.contains('Y'));
        assert!(codes(&review, "HV102_rbcL").contains(&"ambiguity_codes".into()));
        assert!(codes(&review, "HV103_rbcL").contains(&"no_clean_frame".into()));
        assert_eq!(p.records[3].export_id, None);
        assert_eq!(p.records[3].accession.as_deref(), Some("AB123456.1"));
        // The user confirms the detected frames.
        submission::edit(
            &mut s,
            doc,
            SubmissionEdit::SetFrames {
                frames: [("HV101_rbcL", 2), ("HV102_rbcL", 3)]
                    .iter()
                    .map(|(r, c)| FrameChoice {
                        record: (*r).into(),
                        codon_start: *c,
                    })
                    .collect(),
                clear: vec![],
            },
        )
        .unwrap();
        assert_eq!(s.submission(doc).unwrap().frames.len(), 2);
        s.undo(doc).unwrap();
        assert_eq!(s.submission(doc).unwrap().frames, Vec::new());
        s.redo(doc).unwrap();
        assert_eq!(s.submission(doc).unwrap().frames.len(), 2);
        // A refused edit changes nothing.
        assert!(
            submission::edit(
                &mut s,
                doc,
                SubmissionEdit::SetFrames {
                    frames: vec![FrameChoice {
                        record: "HV101_rbcL".into(),
                        codon_start: 4
                    }],
                    clear: vec![]
                },
            )
            .is_err()
        );
        let review = submission::review(&s, doc, &submitter()).unwrap();
        for record in ["HV101_rbcL", "HV102_rbcL"] {
            let c = codes(&review, record);
            assert!(!c.contains(&"frame_unconfirmed".into()), "{c:?}");
            assert!(!c.contains(&"internal_stop".into()), "{c:?}");
        }
        s.save(doc, None).unwrap();
    }
    let mut s = Session::unlogged(root);
    let doc = s
        .open(&root.join("submissions/rbcL.submission.json"))
        .unwrap();
    assert!(s.is_submission(doc));
    assert_eq!(s.submission(doc).unwrap().frames.len(), 2);
    assert_eq!(s.submission(doc).unwrap().batch.genetic_code, Some(11));
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

/// A coding marker is read in one genetic code, which the batch states. A
/// new submission has none, and nothing falls back to a default.
#[test]
fn coding_marker_without_a_genetic_code_is_blocked() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    let SubmissionEdit::SetBatch { mut batch } = rbcl_batch() else {
        unreachable!()
    };
    batch.genetic_code = None;
    submission::edit(&mut s, doc, SubmissionEdit::SetBatch { batch }).unwrap();
    let review = submission::review(&s, doc, &submitter()).unwrap();
    assert!(
        review
            .prepared
            .issues
            .iter()
            .any(|issue| issue.code == "genetic_code"
                && issue.message == "Choose the genetic code for this coding marker"),
        "{:?}",
        review.prepared.issues
    );
    // No record is read in any frame until the code is chosen.
    assert_eq!(review.prepared.records.len(), 0);
    let fresh = submission::create(
        &mut s,
        Path::new("submissions/fresh.submission.json"),
        Path::new("rbcL.afa"),
    )
    .unwrap();
    assert_eq!(s.submission(fresh).unwrap().batch.genetic_code, None);
}

#[test]
fn changed_source_is_refused_until_chosen_again() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    std::fs::write(root.join("rbcL.afa"), ">HV101_rbcL\nACGTACGT\n").unwrap();
    let review = submission::review(&s, doc, &submitter()).unwrap();
    assert_eq!(review.prepared.records.len(), 0);
    assert!(
        review
            .prepared
            .issues
            .iter()
            .any(|i| i.code == "source_changed")
    );
    let source = submission::source_for(root, Path::new("rbcL.afa")).unwrap();
    submission::edit(&mut s, doc, SubmissionEdit::SetSource { source }).unwrap();
    let review = submission::review(&s, doc, &submitter()).unwrap();
    assert_eq!(review.prepared.records.len(), 1);
}

#[test]
fn package_is_deterministic_and_validation_is_tied_to_its_inputs() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    // Without a submitter, the package is a draft listing that action.
    let draft = submission::export(
        &mut s,
        doc,
        Path::new("submissions/rbcL"),
        &Submitter::default(),
    )
    .unwrap();
    assert_eq!(draft.status, "draft");
    let readme = std::fs::read_to_string(root.join("submissions/rbcL/README.txt")).unwrap();
    assert!(readme.contains("Submitter email is not set"), "{readme}");
    assert!(readme.contains("not acceptance by GenBank"));
    let package = root.join("submissions/rbcL");
    let first = submission::export(&mut s, doc, &package, &submitter()).unwrap();
    let bytes = |name: &str| std::fs::read(package.join(name)).unwrap();
    let before: Vec<Vec<u8>> = [
        "package.json",
        "README.txt",
        "genbank/submission.fsa",
        "genbank/submission.src",
        "genbank/submission.tbl",
        "genbank/submission.sbt",
        "records.tsv",
        "coordinate-map.tsv",
        "specimens.tsv",
    ]
    .iter()
    .map(|n| bytes(n))
    .collect();
    let second = submission::export(&mut s, doc, &package, &submitter()).unwrap();
    assert_eq!(first.input_hash, second.input_hash);
    let after: Vec<Vec<u8>> = [
        "package.json",
        "README.txt",
        "genbank/submission.fsa",
        "genbank/submission.src",
        "genbank/submission.tbl",
        "genbank/submission.sbt",
        "records.tsv",
        "coordinate-map.tsv",
        "specimens.tsv",
    ]
    .iter()
    .map(|n| bytes(n))
    .collect();
    assert_eq!(before, after);
    if let Some(capture) = std::env::var_os("BASECRAFT_CAPTURE_SUBMISSION_RECORDS") {
        let capture = Path::new(&capture);
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("submission-package.json"), &before[0]).unwrap();
    }

    let fsa = String::from_utf8(bytes("genbank/submission.fsa")).unwrap();
    assert!(
        fsa.starts_with(">HV101_rbcL [mol_type=genomic DNA] [location=chloroplast] [gcode=11]\n"),
        "{fsa}"
    );
    assert!(!fsa.contains("REF_rbcL"));
    let tbl = String::from_utf8(bytes("genbank/submission.tbl")).unwrap();
    assert!(
        tbl.contains(">Feature HV101_rbcL\n<1\t>307\tgene\n\t\t\tgene\trbcL\n<1\t>307\tCDS\n"),
        "{tbl}"
    );
    assert!(tbl.contains("\t\t\tcodon_start\t2\n\t\t\ttransl_table\t11\n"));
    let src = String::from_utf8(bytes("genbank/submission.src")).unwrap();
    assert!(src.starts_with("Sequence_ID\torganism\tspecimen_voucher\tcollection_date\tgeo_loc_name\tlat_lon\tcollected_by\n"), "{src}");
    assert!(src.contains("HV101_rbcL\tCaulerpa taxifolia\tMEL:HV101\t2019-05-12\tAustralia: Victoria, Port Phillip Bay\t38.1000 S 144.9000 E\tH. Verbruggen\n"), "{src}");
    let map = String::from_utf8(bytes("coordinate-map.tsv")).unwrap();
    // HV102: two Ns trimmed, so export base 1 is alignment column 3.
    assert!(
        map.contains("HV102_rbcL\tHV102_rbcL\t2\t1\t300\t3\t302\n"),
        "{map}"
    );
    let sbt = String::from_utf8(bytes("genbank/submission.sbt")).unwrap();
    assert!(sbt.contains("title \"Marker sequences for submission tests\""));
    // Validation request: a new folder for these exact inputs.
    let request = submission::validation_request(root, &package).unwrap();
    assert_eq!(request["operation"], submission::VALIDATE_OPERATION);
    assert_eq!(
        request["inputs"]["sequences"][0]["path"],
        "submissions/rbcL/genbank/submission.fsa"
    );
    let destination = request["destinations"]["reports"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        destination.starts_with("submissions/rbcL/validation/")
            && destination.ends_with("/table2asn")
    );
    // Simulate a finished run publishing table2asn's files there.
    let reports = root.join(&destination);
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(reports.join("submission.sqn"), "Seq-submit ::= {}\n").unwrap();
    std::fs::write(reports.join("submission.gbf"), "LOCUS HV101_rbcL\n").unwrap();
    std::fs::write(reports.join("table2asn.log"), "exit code: 0\n").unwrap();
    std::fs::write(reports.join("submission.val"), "Error: valid [SEQ_FEAT.InternalStop] 2 internal stops. FEATURE: CDS: x [lcl|HV103_rbcL:<1->306]\nWarning: valid [SEQ_INST.TerminalNs] N at end BIOSEQ: lcl|HV102_rbcL: raw\n").unwrap();
    let artifacts: Vec<_> = [
        "submission.sqn",
        "submission.gbf",
        "submission.val",
        "table2asn.log",
    ]
    .iter()
    .map(|n| json!({"role":"reports","path":reports.join(n).display().to_string(),"digest":"b3:x"}))
    .collect();
    let mut outcome = json!({"state":"completed","run_id":"run1","data":{"status":"completed","version":"1.28.1094","exit_code":0,"message":"ok"},"artifacts":artifacts});
    submission_run::retain(root, &request, &mut outcome);
    let summary = submission::record_validation(root, &package, &outcome).unwrap();
    assert_eq!(summary.status, "errors");
    if let Some(capture) = std::env::var_os("BASECRAFT_CAPTURE_SUBMISSION_RECORDS") {
        std::fs::write(
            Path::new(&capture).join("submission-validation.json"),
            std::fs::read(
                submission::validation_dir(&package, &summary.input_hash).join("summary.json"),
            )
            .unwrap(),
        )
        .unwrap();
    }

    assert_eq!((summary.errors, summary.warnings), (1, 1));
    let stop = &summary.issues[0];
    assert_eq!(stop.record.as_deref(), Some("HV103_rbcL"));
    assert_eq!(stop.specimen.as_deref(), Some("HV103"));
    assert_eq!(stop.severity, Severity::Blocking);
    assert!(package.join("genbank/submission.sqn").is_file());
    let manifest: serde_json::Value = serde_json::from_slice(&bytes("package.json")).unwrap();
    assert_eq!(manifest["status"], "validation_errors");
    assert_eq!(manifest["tools"]["table2asn"], "1.28.1094");
    // Same inputs: the run still applies after regenerating.
    let again = submission::export(&mut s, doc, &package, &submitter()).unwrap();
    assert_eq!(again.status, "validation_errors");
    assert!(package.join("genbank/submission.sqn").is_file());
    assert!(
        submission::current_validation(root, &package)
            .unwrap()
            .is_some()
    );
    // Changed inputs: the old run no longer applies and its .sqn goes.
    submission::edit(
        &mut s,
        doc,
        SubmissionEdit::SetExport {
            export: basecraft_protocol::submission::ExportSettings {
                trim_terminal_unknown: false,
            },
        },
    )
    .unwrap();
    let changed = submission::export(&mut s, doc, &package, &submitter()).unwrap();
    assert_ne!(changed.input_hash, first.input_hash);
    assert!(!package.join("genbank/submission.sqn").exists());
    assert!(
        submission::current_validation(root, &package)
            .unwrap()
            .is_none()
    );
    // A missing tool is an unavailable check, not a sequence error.
    let mut unavailable = json!({"state":"completed","run_id":"run2","data":{"status":"unavailable","version":"unknown","message":"table2asn is not installed"},"artifacts":[]});
    let request = submission::validation_request(root, &package).unwrap();
    std::fs::create_dir_all(root.join(request["destinations"]["reports"].as_str().unwrap()))
        .unwrap();
    submission_run::retain(root, &request, &mut unavailable);
    let summary = submission::record_validation(root, &package, &unavailable).unwrap();
    assert_eq!(summary.status, "unavailable");
    assert_eq!(summary.issues[0].severity, Severity::Unavailable);
    assert_eq!(summary.errors, 0);
    // Editing the exported files makes the package refuse validation.
    std::fs::write(package.join("genbank/submission.tbl"), "edited").unwrap();
    assert!(submission::validation_request(root, &package).is_err());
    // A folder with other files is never used as a package.
    std::fs::create_dir_all(root.join("other")).unwrap();
    std::fs::write(root.join("other/notes.txt"), "keep").unwrap();
    assert!(submission::export(&mut s, doc, Path::new("other"), &submitter()).is_err());
}

#[test]
fn accessions_import_and_table() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    submission::export(&mut s, doc, &root.join("package"), &submitter()).unwrap();
    let text =
        "Sequence ID\tAccession\nHV101_rbcL\tPX123456\nHV102_rbcL PX123457.1\nunknown PX123458\n";
    let plan = submission::plan_accessions(&s, doc, text, &submitter()).unwrap();
    assert_eq!(plan.accessions.len(), 2);
    assert_eq!(plan.unmatched, vec!["unknown PX123458".to_string()]);
    submission::edit(
        &mut s,
        doc,
        SubmissionEdit::SetAccessions {
            accessions: plan.accessions,
        },
    )
    .unwrap();
    let table =
        String::from_utf8(submission::accession_table(&s, doc, &submitter()).unwrap()).unwrap();
    assert_eq!(
        table,
        "record\tspecimen_id\torganism\texport_id\tdecision\taccession\nHV101_rbcL\tHV101\tCaulerpa taxifolia\tHV101_rbcL\tnew\tPX123456\nHV102_rbcL\tHV102\tCaulerpa taxifolia\tHV102_rbcL\tnew\tPX123457.1\nREF_rbcL\t\t\t\texisting\tAB123456.1\n"
    );
}

#[test]
fn rrna_its_and_coi_batches() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    // ITS: one misc_RNA with a component note; duplicates and unlinked rows.
    let its = submission::create(
        &mut s,
        Path::new("its.submission.json"),
        Path::new("ITS.afa"),
    )
    .unwrap();
    submission::edit(
        &mut s,
        its,
        SubmissionEdit::SetBatch {
            batch: BatchSettings {
                marker: Marker::Its {
                    components: vec![
                        ItsComponent::SmallSubunit,
                        ItsComponent::Its1,
                        ItsComponent::Rrna58S,
                        ItsComponent::Its2,
                    ],
                },
                ..BatchSettings::default()
            },
        },
    )
    .unwrap();
    let review = submission::review(&s, its, &submitter()).unwrap();
    let f = review.prepared.records[0].feature.as_ref().unwrap();
    assert_eq!(f.kind, "misc_RNA");
    assert_eq!(
        f.qualifiers[0].1,
        "contains small subunit ribosomal RNA, internal transcribed spacer 1, 5.8S ribosomal RNA, and internal transcribed spacer 2"
    );
    assert_eq!((f.partial5, f.partial3), (true, true));
    assert!(codes(&review, "HV403_ITS").contains(&"duplicate_record".into()));
    assert!(codes(&review, "orphan_ITS").contains(&"unlinked".into()));
    assert!(review.prepared.issues.iter().any(|i| i.code == "no_title"));
    // 18S: an rRNA; a specimen identified only to genus gets its voucher.
    let ssu = submission::create(
        &mut s,
        Path::new("18S.submission.json"),
        Path::new("18S.afa"),
    )
    .unwrap();
    submission::edit(
        &mut s,
        ssu,
        SubmissionEdit::SetBatch {
            batch: BatchSettings {
                marker: Marker::Rrna {
                    product: "18S ribosomal RNA".into(),
                },
                ..BatchSettings::default()
            },
        },
    )
    .unwrap();
    let review = submission::review(&s, ssu, &submitter()).unwrap();
    assert_eq!(
        review.prepared.records[1].organism.as_deref(),
        Some("Caulerpa sp. HV302")
    );
    assert_eq!(
        review.prepared.records[0].feature.as_ref().unwrap().kind,
        "rRNA"
    );
    // COI under the invertebrate mitochondrial code; missing date reasons.
    let coi = submission::create(
        &mut s,
        Path::new("coi.submission.json"),
        Path::new("COI.afa"),
    )
    .unwrap();
    submission::edit(
        &mut s,
        coi,
        SubmissionEdit::SetBatch {
            batch: BatchSettings {
                marker: Marker::Coding {
                    gene: "COI".into(),
                    product: "cytochrome c oxidase subunit I".into(),
                },
                organelle: Some(Organelle::Mitochondrion),
                genetic_code: Some(5),
                ..BatchSettings::default()
            },
        },
    )
    .unwrap();
    let review = submission::review(&s, coi, &submitter()).unwrap();
    for r in &review.prepared.records {
        assert_eq!(r.frame.as_ref().unwrap().detected, Some(2), "{}", r.record);
    }
    assert!(!codes(&review, "HV203_COI").contains(&"missing_source".into()));
    assert!(codes(&review, "HV204_COI").contains(&"missing_source".into()));
    let hv203 = &review.prepared.records[2];
    assert!(
        hv203
            .qualifiers
            .contains(&("collection_date".into(), "not collected".into()))
    );
    submission::edit(
        &mut s,
        coi,
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "HV204_COI".into(),
                decision: Decision::Exclude,
            }],
        },
    )
    .unwrap();
    submission::edit(&mut s, coi, publication()).unwrap();
    confirm_selection(&mut s, coi);
    let package = submission::export(&mut s, coi, Path::new("coi"), &submitter()).unwrap();
    let fsa = std::fs::read_to_string(root.join("coi/genbank/submission.fsa")).unwrap();
    assert!(fsa.contains("[location=mitochondrion] [mgcode=5]"), "{fsa}");
    assert_eq!(package.records, 3);
}

#[test]
fn selection_confirmation_blocks_export_and_survives_undo_reload_and_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("a.afa"),
        ">sample\nACGT\n>gb|MZ123456.1|reference\nACGT\n",
    )
    .unwrap();
    {
        let mut s = Session::new(root);
        let doc =
            submission::create(&mut s, Path::new("a.submission.json"), Path::new("a.afa")).unwrap();
        let review = submission::review(&s, doc, &submitter()).unwrap();
        assert!(!review.selection_confirmed);
        assert_eq!(review.prepared.records[0].decision, Decision::New);
        assert_eq!(review.prepared.records[1].decision, Decision::Exclude);
        assert_eq!(
            review.prepared.records[1].accession_candidates[0].accession,
            "MZ123456.1"
        );
        assert!(submission::export(&mut s, doc, Path::new("package"), &submitter()).is_err());
        assert!(
            !root.join("package").exists(),
            "unconfirmed export writes nothing"
        );
        confirm_selection(&mut s, doc);
        assert!(
            submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        s.undo(doc).unwrap();
        assert!(
            !submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        s.redo(doc).unwrap();
        assert!(
            submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        submission::edit(&mut s, doc, publication()).unwrap();
        assert!(
            submission::review(&s, doc, &Submitter::default())
                .unwrap()
                .selection_confirmed,
            "unrelated publication/contact edits retain confirmation"
        );
        submission::edit(
            &mut s,
            doc,
            SubmissionEdit::SetRows {
                rows: vec![RowDecision {
                    record: "gb|MZ123456.1|reference".into(),
                    decision: Decision::New,
                }],
            },
        )
        .unwrap();
        assert!(
            !submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        s.undo(doc).unwrap();
        assert!(
            submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        s.redo(doc).unwrap();
        assert!(
            !submission::review(&s, doc, &submitter())
                .unwrap()
                .selection_confirmed
        );
        // A stale review fingerprint cannot authorize this changed selection.
        submission::edit(
            &mut s,
            doc,
            SubmissionEdit::ConfirmSelection {
                selection_hash: review.selection_hash.unwrap(),
            },
        )
        .unwrap();
        assert!(submission::export(&mut s, doc, Path::new("package"), &submitter()).is_err());
        confirm_selection(&mut s, doc);
        let package = submission::export(&mut s, doc, Path::new("package"), &submitter()).unwrap();
        assert_eq!(
            package.records, 2,
            "explicit inclusion overrides accession default"
        );
        s.save(doc, None).unwrap();
    }
    let mut s = Session::unlogged(root);
    let doc = s.open(&root.join("a.submission.json")).unwrap();
    assert!(
        submission::review(&s, doc, &submitter())
            .unwrap()
            .selection_confirmed
    );
    let replay = records::replay(root).unwrap();
    assert!(replay.ok(), "{:#?}", replay.problems);
    std::fs::write(
        root.join("a.afa"),
        ">sample\nACGA\n>gb|MZ123456.1|reference\nACGT\n",
    )
    .unwrap();
    assert!(
        !submission::review(&s, doc, &submitter())
            .unwrap()
            .selection_confirmed
    );
    assert!(submission::export(&mut s, doc, Path::new("package"), &submitter()).is_err());
    let source = submission::source_for(root, Path::new("a.afa")).unwrap();
    submission::edit(&mut s, doc, SubmissionEdit::SetSource { source }).unwrap();
    assert!(
        !submission::review(&s, doc, &submitter())
            .unwrap()
            .selection_confirmed
    );
    assert_eq!(
        submission::review(&s, doc, &submitter())
            .unwrap()
            .prepared
            .records[1]
            .decision,
        Decision::New
    );
}

#[test]
fn stored_accession_changes_invalidate_confirmation_but_preserve_explicit_choices() {
    use basecraft_protocol::metadata::{Cell, CellChange, ChangeSet};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    let metadata_doc = metadata::open(&mut s, false).unwrap().unwrap();
    metadata::apply_plan(
        &mut s,
        metadata_doc,
        ChangeSet {
            cells: vec![CellChange {
                specimen: "HV101".into(),
                column: "catalogNumber".into(),
                value: Some(Cell::Text("MZ123456.1".into())),
            }],
            ..ChangeSet::default()
        },
    )
    .unwrap();
    s.save(metadata_doc, None).unwrap();
    let review = submission::review(&s, doc, &submitter()).unwrap();
    assert!(!review.selection_confirmed);
    let row = &review.prepared.records[0];
    assert_eq!(
        row.decision,
        Decision::New,
        "keep the explicit user inclusion"
    );
    assert!(
        row.accession_candidates
            .iter()
            .any(|c| c.source == "specimen HV101, catalogNumber")
    );
    confirm_selection(&mut s, doc);
    submission::edit(
        &mut s,
        doc,
        SubmissionEdit::SetAccessions {
            accessions: vec![basecraft_protocol::submission::AccessionEntry {
                record: "HV102_rbcL".into(),
                accession: "PX123457".into(),
            }],
        },
    )
    .unwrap();
    assert!(
        !submission::review(&s, doc, &submitter())
            .unwrap()
            .selection_confirmed
    );
    assert_eq!(
        submission::review(&s, doc, &submitter())
            .unwrap()
            .prepared
            .records[1]
            .decision,
        Decision::New
    );
}

fn inclusion_rule() -> basecraft_protocol::submission::InclusionRule {
    use basecraft_protocol::submission::*;
    InclusionRule {
        name: "fourth field".into(),
        input: IdentifierSource::Field {
            separator: "_".into(),
            index: 4,
        },
        mode: MatchMode::Exact,
        pattern: "XX".into(),
        case_sensitive: true,
        trim: true,
        matching: InclusionAction::Include,
        nonmatching: InclusionAction::Exclude,
        unresolved: InclusionAction::Unchanged,
        override_manual: false,
    }
}

#[test]
fn inclusion_preview_is_atomic_stale_safe_undoable_and_replayable() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let fasta = root.path().join("names.fas");
    std::fs::write(
        &fasta,
        ">a_b_HV00145_XX desc_not_part_of_ID\nACGT\n>a_b_c_AM049955\nACGT\n>missing\nACGT\n",
    )
    .unwrap();
    let mut session = Session::new(root.path());
    let doc = submission::create(&mut session, Path::new("test.submission.json"), &fasta).unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetBatch {
            batch: BatchSettings {
                marker: Marker::Rrna {
                    product: "18S ribosomal RNA".into(),
                },
                ..BatchSettings::default()
            },
        },
    )
    .unwrap();
    assert!(
        session
            .submission(doc)
            .unwrap()
            .marker_drafts
            .contains_key("rrna")
    );
    let plan = submission::plan_inclusion(&session, doc, &inclusion_rule()).unwrap();
    assert_eq!((plan.matching, plan.unresolved, plan.included), (1, 1, 2));
    assert_eq!(plan.rows[0].record, "a_b_HV00145_XX");
    let before = session.submission(doc).unwrap().clone();
    let mut forged = plan.clone();
    forged.rows[0].proposed = Decision::Exclude;
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::ApplyInclusion {
                plan: Box::new(forged)
            }
        )
        .is_err()
    );
    assert_eq!(session.submission(doc).unwrap(), &before);
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::ApplyInclusion {
            plan: Box::new(plan.clone()),
        },
    )
    .unwrap();
    let after = session.submission(doc).unwrap().clone();
    assert_eq!(
        after.rows.len(),
        2,
        "Unresolved unchanged row must not become explicit"
    );
    session.undo(doc).unwrap();
    assert_eq!(session.submission(doc).unwrap(), &before);
    session.redo(doc).unwrap();
    assert_eq!(session.submission(doc).unwrap(), &after);
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::ApplyInclusion {
                plan: Box::new(plan)
            }
        )
        .is_err()
    );
    session.save(doc, None).unwrap();
    assert!(records::replay(root.path()).unwrap().ok());
    let plan = submission::plan_inclusion(&session, doc, &inclusion_rule()).unwrap();
    std::fs::write(&fasta, ">changed\nACGT\n").unwrap();
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::ApplyInclusion {
                plan: Box::new(plan)
            }
        )
        .is_err()
    );
    assert_eq!(session.submission(doc).unwrap(), &after);
    assert!(
        records::replay(root.path()).unwrap().ok(),
        "Historical replay must not read changed source"
    );
}

#[test]
fn inclusion_rejects_metadata_changes_and_preserves_manual_origins() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let mut session = Session::new(root.path());
    let doc = submission::create(
        &mut session,
        Path::new("test.submission.json"),
        Path::new("18S.afa"),
    )
    .unwrap();
    let review = submission::review(&session, doc, &submitter()).unwrap();
    let name = review.prepared.records[0].record.clone();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: name.clone(),
                decision: Decision::Exclude,
            }],
        },
    )
    .unwrap();
    let mut rule = inclusion_rule();
    rule.input = basecraft_protocol::submission::IdentifierSource::Identifier;
    rule.pattern = String::new();
    rule.mode = basecraft_protocol::submission::MatchMode::Contains;
    let plan = submission::plan_inclusion(&session, doc, &rule).unwrap();
    assert!(
        plan.rows
            .iter()
            .find(|r| r.record == name)
            .unwrap()
            .preserved_manual
    );
    let meta = metadata::manifest_path(root.path());
    let bytes = std::fs::read(&meta).unwrap();
    let mut changed = bytes.clone();
    changed.push(b' ');
    std::fs::write(&meta, changed).unwrap();
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::ApplyInclusion {
                plan: Box::new(plan)
            }
        )
        .is_err()
    );
}

#[test]
fn cached_review_invalidates_source_metadata_and_relevant_settings() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let mut session = Session::new(root.path());
    let doc = submission::create(
        &mut session,
        Path::new("test.submission.json"),
        Path::new("18S.afa"),
    )
    .unwrap();
    let mut cache = submission::ReviewCache::default();
    let mut sub = session.submission(doc).unwrap().clone();
    let first =
        submission::review_submission_cached(root.path(), &sub, &submitter(), &mut cache).unwrap();
    sub.publication = basecraft_protocol::submission::Publication {
        title: "Now named".into(),
        authors: basecraft_docs::submission::parse_authors("Doe, Jane").unwrap(),
    };
    let next =
        submission::review_submission_cached(root.path(), &sub, &submitter(), &mut cache).unwrap();
    assert_eq!(first.prepared.records, next.prepared.records);
    assert!(
        !next
            .prepared
            .issues
            .iter()
            .any(|i| i.code == "no_title" || i.code == "no_authors")
    );
    let plan = submission::plan_inclusion(&session, doc, &inclusion_rule()).unwrap();
    let taxa = root.path().join("metadata/taxa.tsv");
    let old_taxa = std::fs::read_to_string(&taxa).unwrap();
    assert!(old_taxa.contains("Caulerpa webbiana"));
    std::fs::write(
        &taxa,
        old_taxa.replace("Caulerpa webbiana", "Caulerpa changed"),
    )
    .unwrap();
    let external =
        submission::review_submission_cached(root.path(), &sub, &submitter(), &mut cache).unwrap();
    assert_ne!(
        external.prepared.records, next.prepared.records,
        "Externally edited TSV invalidates cache without manifest edit"
    );
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::ApplyInclusion {
                plan: Box::new(plan)
            }
        )
        .is_err(),
        "External TSV edits invalidate inclusion evidence"
    );
    sub.batch.marker = Marker::Spacer {
        description: "Region".into(),
    };
    let next =
        submission::review_submission_cached(root.path(), &sub, &submitter(), &mut cache).unwrap();
    assert!(
        next.prepared
            .exported()
            .all(|r| r.feature.as_ref().unwrap().kind == "misc_feature")
    );
    std::fs::write(root.path().join("18S.afa"), ">changed\nACTG\n").unwrap();
    let next =
        submission::review_submission_cached(root.path(), &sub, &submitter(), &mut cache).unwrap();
    assert_eq!(next.prepared.records, []);
    assert!(
        next.prepared
            .issues
            .iter()
            .any(|i| i.code == "source_changed")
    );
}

#[test]
fn distinct_details_export_undo_replay_and_save_failure_keep_memory_recoverable() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let mut session = Session::new(root.path());
    let doc = rbcl(&mut session);
    let submission = Publication {
        title: "Submission study".into(),
        authors: basecraft_docs::submission::parse_authors("van der Meer, Anne M.").unwrap(),
    };
    let publication = Publication {
        title: "Paper study".into(),
        authors: basecraft_docs::submission::parse_authors("O'Neill-Smith, Bob").unwrap(),
    };
    let before = session.submission(doc).unwrap().clone();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetDetails {
            submission: submission.clone(),
            publication: publication.clone(),
            publication_differs: true,
        },
    )
    .unwrap();
    session.save(doc, None).unwrap();
    let package = root.path().join("distinct");
    submission::export(&mut session, doc, &package, &submitter()).unwrap();
    let sbt = std::fs::read_to_string(package.join("genbank/submission.sbt")).unwrap();
    let (direct, paper) = sbt.split_once("Seqdesc ::=").unwrap();
    assert!(direct.contains("descr \"Submission study\""));
    assert!(direct.contains("van der Meer"));
    assert!(!direct.contains("O'Neill-Smith"));
    assert!(paper.contains("title \"Paper study\""));
    assert!(paper.contains("O'Neill-Smith"));
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetDetails {
            submission,
            publication: publication.clone(),
            publication_differs: false,
        },
    )
    .unwrap();
    let changed = session.submission(doc).unwrap().clone();
    let target = root.path().join("submissions/rbcL.submission.json");
    let disk = std::fs::read(&target).unwrap();
    std::fs::remove_file(&target).unwrap();
    std::fs::create_dir(&target).unwrap();
    assert!(session.save(doc, None).is_err());
    assert_eq!(
        session.submission(doc).unwrap(),
        &changed,
        "Failed save must retain recoverable edited state"
    );
    assert_eq!(
        session.submission(doc).unwrap().publication,
        publication,
        "Disabled custom details retained"
    );
    std::fs::remove_dir(&target).unwrap();
    std::fs::write(&target, disk).unwrap();
    session.undo(doc).unwrap();
    assert!(session.submission(doc).unwrap().publication_differs);
    // Export adds its own typed mapping, so undoing details must retain that mapping.
    assert_eq!(session.submission(doc).unwrap().rows, before.rows);
    assert!(records::replay(root.path()).unwrap().ok());
}

#[test]
fn inclusion_presets_reuse_the_open_metadata_owner_and_preserve_pending_edits() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path());
    let doc = metadata::open(&mut session, true).unwrap().unwrap();
    session
        .apply(
            doc,
            basecraft_protocol::Op::MetadataEdit {
                edit: Box::new(basecraft_protocol::metadata::MetadataEdit::Apply {
                    changes: basecraft_protocol::metadata::ChangeSet {
                        specimens: vec!["pending-specimen".into()],
                        ..Default::default()
                    },
                }),
            },
        )
        .unwrap();
    assert_eq!(metadata::open(&mut session, false).unwrap(), Some(doc));
    let rule = inclusion_rule();
    submission::save_inclusion_preset(&mut session, rule.clone()).unwrap();
    assert_eq!(
        session.metadata(doc).unwrap().specimens.rows[0].id,
        "pending-specimen"
    );
    assert_eq!(
        session.metadata(doc).unwrap().import.inclusion_rules,
        [rule]
    );
    let saved = metadata::load(&metadata::manifest_path(root.path())).unwrap();
    assert_eq!(saved.metadata, *session.metadata(doc).unwrap());
    session.undo(doc).unwrap();
    assert_eq!(
        session.metadata(doc).unwrap().specimens.rows[0].id,
        "pending-specimen"
    );
    assert_eq!(session.metadata(doc).unwrap().import.inclusion_rules, []);
    assert!(records::replay(root.path()).unwrap().ok());
}

#[test]
fn parsed_full_middle_names_and_initials_reach_distinct_export_citations() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let mut session = Session::new(root.path());
    let doc = submission::create(
        &mut session,
        Path::new("names.submission.json"),
        Path::new("18S.afa"),
    )
    .unwrap();
    let submission_authors = basecraft_docs::submission::parse_authors(
        "van der Meer, Anne Marie\nO'Neill-Smith, J. R.\nde la Cruz, Jean-Pierre Marie-Claire",
    )
    .unwrap();
    let publication_authors =
        basecraft_docs::submission::parse_authors("Smith-Jones, A. M.R.\nLegacy, A. MR").unwrap();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetDetails {
            submission: Publication {
                title: "Sequence study".into(),
                authors: submission_authors.clone(),
            },
            publication: Publication {
                title: "Paper study".into(),
                authors: publication_authors,
            },
            publication_differs: true,
        },
    )
    .unwrap();
    session.save(doc, None).unwrap();
    let stored = submission::load(&root.path().join("names.submission.json")).unwrap();
    assert_eq!(stored.submission_citation().authors, submission_authors);
    assert_eq!(stored.submission_citation().authors[0].middle, "Marie");
    confirm_selection(&mut session, doc);
    let package = root.path().join("package");
    submission::export(&mut session, doc, &package, &submitter()).unwrap();
    let text = std::fs::read_to_string(package.join("genbank/submission.sbt")).unwrap();
    let (direct, paper) = text.split_once("Seqdesc ::=").unwrap();
    for expected in [
        "last \"van der Meer\"",
        "middle \"Marie\"",
        "initials \"A.M.\"",
        "last \"O'Neill-Smith\"",
        "initials \"J.R.\"",
        "middle \"Marie-Claire\"",
        "initials \"J.-P.M.-C.\"",
    ] {
        assert!(direct.contains(expected), "Missing {expected} in {direct}");
    }
    assert!(paper.contains("last \"Smith-Jones\""));
    assert!(paper.contains("initials \"A.M.R.\""));
    assert!(!paper.contains("middle "));
    assert!(!text.contains("A.M.a.r.i.e."));
    let replayed = records::replay(root.path()).unwrap();
    assert!(replayed.ok(), "{:?}", replayed.problems);
}

#[test]
fn rejected_edit_and_wrong_save_format_preserve_submission_undo_redo() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let mut session = Session::new(root.path());
    let doc = rbcl(&mut session);
    let before = session.submission(doc).unwrap().clone();
    let mut changed = before.publication.clone();
    changed.title = "Changed publication for undo compatibility".into();
    submission::edit(
        &mut session,
        doc,
        SubmissionEdit::SetPublication {
            publication: changed.clone(),
        },
    )
    .unwrap();
    let edited = session.submission(doc).unwrap().clone();
    assert_ne!(edited, before);
    let revision = session.revision(doc).unwrap();
    let hashes = session.save_hashes(doc).unwrap();

    assert!(
        session
            .apply(
                doc,
                basecraft_protocol::Op::AlnRenameRow {
                    row: 0,
                    name: "wrong kind".into(),
                }
            )
            .is_err()
    );
    assert_eq!(session.submission(doc).unwrap(), &edited);
    assert_eq!(session.revision(doc).unwrap(), revision);
    assert_eq!(session.save_hashes(doc).unwrap(), hashes);

    changed.title = "invalid\npublication".into();
    assert!(
        submission::edit(
            &mut session,
            doc,
            SubmissionEdit::SetPublication {
                publication: changed,
            }
        )
        .is_err()
    );
    assert_eq!(session.submission(doc).unwrap(), &edited);
    assert_eq!(session.revision(doc).unwrap(), revision);

    let wrong_target = root.path().join("wrong.fasta");
    assert!(session.save(doc, Some(&wrong_target)).is_err());
    assert!(!wrong_target.exists());
    assert_eq!(session.submission(doc).unwrap(), &edited);
    session.undo(doc).unwrap();
    assert_eq!(session.submission(doc).unwrap(), &before);
    session.redo(doc).unwrap();
    assert_eq!(session.submission(doc).unwrap(), &edited);
    session.save(doc, None).unwrap();
    assert!(records::replay(root.path()).unwrap().ok());
}
