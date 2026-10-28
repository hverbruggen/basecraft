//! GenBank marker submissions: the document, review, package, validation
//! records, accessions, undo and replay.
use std::path::{Path, PathBuf};

use basecraft_docs::submission::Severity;
use basecraft_ops::metadata::{self, ImportOptions, SheetImport};
use basecraft_ops::submission::{self, Review};
use basecraft_ops::{DocId, Session, replay};
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
            genetic_code: 11,
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
    doc
}

#[test]
fn coding_review_frames_undo_save_reload_and_replay() {
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
        assert_eq!(s.submission(doc).frames.len(), 2);
        s.undo(doc).unwrap();
        assert!(s.submission(doc).frames.is_empty());
        s.redo(doc).unwrap();
        assert_eq!(s.submission(doc).frames.len(), 2);
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
    assert_eq!(s.submission(doc).frames.len(), 2);
    assert_eq!(s.submission(doc).batch.genetic_code, 11);
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

#[test]
fn changed_source_is_refused_until_chosen_again() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
    std::fs::write(root.join("rbcL.afa"), ">HV101_rbcL\nACGTACGT\n").unwrap();
    let review = submission::review(&s, doc, &submitter()).unwrap();
    assert!(review.prepared.records.is_empty());
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
    std::fs::write(reports.join("submission.val"), "Error: valid [SEQ_FEAT.InternalStop] 2 internal stops. FEATURE: CDS: x [lcl|HV103_rbcL:<1->306]\nWarning: valid [SEQ_INST.TerminalNs] N at end BIOSEQ: lcl|HV102_rbcL: raw\n").unwrap();
    let artifacts: Vec<_> = ["submission.sqn", "submission.gbf", "submission.val"].iter().map(|n| json!({"role":"reports","path":reports.join(n).display().to_string(),"digest":"b3:x"})).collect();
    let outcome = json!({"state":"completed","run_id":"run1","data":{"status":"completed","version":"1.28.1094","message":"ok"},"artifacts":artifacts});
    let summary = submission::record_validation(root, &package, &outcome).unwrap();
    assert_eq!(summary.status, "errors");
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
    let unavailable = json!({"state":"completed","run_id":"run2","data":{"status":"unavailable","version":"unknown","message":"table2asn is not installed"},"artifacts":[]});
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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let mut s = Session::new(root);
    let doc = rbcl(&mut s);
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
    assert!((f.partial5, f.partial3) == (true, true));
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
                genetic_code: 5,
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
    let package = submission::export(&mut s, coi, Path::new("coi"), &submitter()).unwrap();
    let fsa = std::fs::read_to_string(root.join("coi/genbank/submission.fsa")).unwrap();
    assert!(fsa.contains("[location=mitochondrion] [mgcode=5]"), "{fsa}");
    assert_eq!(package.records, 3);
}
