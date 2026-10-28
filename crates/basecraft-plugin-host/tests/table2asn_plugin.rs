//! table2asn validation through the registered runner: package export,
//! the `basecraft.table2asn/validate@1` job and recording its reports.
//!
//! The bundled plugin runs against a stand-in table2asn script in every
//! test run. With `BASECRAFT_TEST_TABLE2ASN` set to a real table2asn, the
//! fixture packages are also validated by it; `BASECRAFT_TEST_EVIDENCE`
//! names a folder to copy the packages and reports to.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use basecraft_ops::Session;
use basecraft_ops::metadata::{self, ImportOptions, SheetImport};
use basecraft_ops::submission::{self, ValidationSummary};
use basecraft_platform::settings::Submitter;
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::OperationRequest;
use basecraft_protocol::submission::{
    Author, BatchSettings, Decision, FrameChoice, ItsComponent, Marker, Organelle, Publication,
    RowDecision, SubmissionEdit,
};

const FAKE: &str = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "table2asn: 0.0.test"; exit 0; fi
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "-outdir" ]; then out="$2"; shift; fi
  shift
done
id=$(grep -m1 '^>' submission.fsa | cut -c2- | cut -d' ' -f1)
echo "Seq-submit ::= {}" > "$out/submission.sqn"
printf 'Error: valid [SEQ_DESCR.BadGeoLocNameCode] Bad geo_loc_name [Atlantis] DESCRIPTOR: BioSrc: x BIOSEQ: lcl|%s: raw, dna len= 10\n' "$id" > "$out/submission.val"
printf 'Total messages:\t\t1\n' > "$out/submission.stats"
echo "LOCUS $id" > "$out/submission.gbf"
"#;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/submission")
}

/// The bundled plugin, run with `BASECRAFT_TABLE2ASN` set to `tool`.
fn registry(root: &Path, tool: &Path) -> Registry {
    let dir = root.join("bundle/table2asn");
    std::fs::create_dir_all(&dir).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/table2asn");
    for file in ["plugin.toml", "operations.json"] {
        std::fs::copy(source.join(file), dir.join(file)).unwrap();
    }
    std::fs::copy(
        source.join("table2asn_plugin.py"),
        dir.join("real_plugin.py"),
    )
    .unwrap();
    std::fs::write(
        dir.join("table2asn_plugin.py"),
        format!(
            "import os, pathlib, runpy\nos.environ['BASECRAFT_TABLE2ASN'] = {:?}\nrunpy.run_path(str(pathlib.Path(__file__).with_name('real_plugin.py')), run_name='__main__')\n",
            tool.display().to_string()
        ),
    )
    .unwrap();
    Registry::load(&RegistryConfig {
        bundled: root.join("bundle"),
        packages: root.join("packages"),
        index: root.join("plugins.json"),
    })
    .unwrap()
}

fn project(root: &Path) {
    for name in ["rbcL.afa", "COI.afa", "18S.afa", "ITS.afa", "specimens.csv"] {
        std::fs::copy(fixtures().join(name), root.join(name)).unwrap();
    }
    let mut s = Session::unlogged(root);
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
        street: "Royal Parade".into(),
        city: "Melbourne".into(),
        state: "Victoria".into(),
        postal_code: "3010".into(),
        country: "Australia".into(),
        ..Submitter::default()
    }
}

/// Export a package for `alignment` with `edits` and validate it.
fn validate(
    root: &Path,
    registry: &Registry,
    name: &str,
    alignment: &str,
    edits: Vec<SubmissionEdit>,
) -> ValidationSummary {
    let mut s = Session::unlogged(root);
    let doc = submission::create(
        &mut s,
        Path::new(&format!("{name}.submission.json")),
        Path::new(alignment),
    )
    .unwrap();
    let publication = SubmissionEdit::SetPublication {
        publication: Publication {
            title: "Marker sequences for submission tests".into(),
            authors: vec![Author {
                first: "Jane".into(),
                last: "Doe".into(),
                middle: String::new(),
            }],
        },
    };
    for edit in std::iter::once(publication).chain(edits) {
        submission::edit(&mut s, doc, edit).unwrap();
    }
    let selection_hash = submission::review(&s, doc, &submitter())
        .unwrap()
        .selection_hash
        .unwrap();
    submission::edit(
        &mut s,
        doc,
        SubmissionEdit::ConfirmSelection { selection_hash },
    )
    .unwrap();
    submission::export(&mut s, doc, Path::new(name), &submitter()).unwrap();
    let request: OperationRequest =
        serde_json::from_value(submission::validation_request(root, Path::new(name)).unwrap())
            .unwrap();
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            registry,
            request,
            SessionPolicy::new(root, true).unwrap(),
            "test",
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let outcome = loop {
        let outcome = jobs.get(&accepted.run_id).unwrap();
        if outcome.state.terminal() {
            break outcome;
        }
        assert!(Instant::now() < deadline, "job did not finish");
        std::thread::sleep(Duration::from_millis(20));
    };
    let outcome = serde_json::to_value(outcome).unwrap();
    submission::record_validation(root, Path::new(name), &outcome)
        .unwrap_or_else(|e| panic!("{e}: {outcome:#}"))
}

fn its_edits() -> Vec<SubmissionEdit> {
    vec![
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
        SubmissionEdit::SetRows {
            rows: vec![RowDecision {
                record: "orphan_ITS".into(),
                decision: Decision::Exclude,
            }],
        },
    ]
}

#[test]
fn registered_validation_maps_reports_and_reports_a_missing_tool_as_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    project(&root);
    let fake = temp.path().join("table2asn");
    std::fs::write(&fake, FAKE).unwrap();
    let mut permissions = std::fs::metadata(&fake).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&fake, permissions).unwrap();
    let summary = validate(
        &root,
        &registry(&temp.path().join("ok"), &fake),
        "its",
        "ITS.afa",
        its_edits(),
    );
    assert_eq!(summary.status, "errors", "{summary:#?}");
    assert_eq!(summary.tool_version, "0.0.test");
    assert_eq!(summary.issues[0].record.as_deref(), Some("HV401_ITS"));
    assert_eq!(summary.issues[0].specimen.as_deref(), Some("HV401"));
    assert!(root.join("its/genbank/submission.sqn").is_file());
    let reports = root.join("its").join(&summary.directory);
    assert!(
        reports.join("table2asn.log").is_file(),
        "{}",
        reports.display()
    );
    // A missing executable: unavailable, nothing about the sequences.
    let summary = validate(
        &root,
        &registry(
            &temp.path().join("missing"),
            &temp.path().join("no-table2asn"),
        ),
        "its2",
        "ITS.afa",
        its_edits(),
    );
    assert_eq!(summary.status, "unavailable", "{summary:#?}");
    assert_eq!(summary.errors, 0);
    assert!(!root.join("its2/genbank/submission.sqn").exists());
}

/// Real table2asn on the fixture packages, when one is configured.
#[test]
fn real_table2asn_on_fixture_packages() {
    let Some(tool) = std::env::var_os("BASECRAFT_TEST_TABLE2ASN") else {
        eprintln!("BASECRAFT_TEST_TABLE2ASN is not set; skipping the real table2asn run");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    project(&root);
    let registry = registry(&temp.path().join("bundle-root"), Path::new(&tool));
    let frames = |records: &[&str], codon_start: u8| SubmissionEdit::SetFrames {
        frames: records
            .iter()
            .map(|r| FrameChoice {
                record: (*r).into(),
                codon_start,
            })
            .collect(),
        clear: vec![],
    };
    let rbcl = validate(
        &root,
        &registry,
        "rbcL",
        "rbcL.afa",
        vec![
            SubmissionEdit::SetBatch {
                batch: BatchSettings {
                    marker: Marker::Coding {
                        gene: "rbcL".into(),
                        product: "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit"
                            .into(),
                    },
                    organelle: Some(Organelle::Chloroplast),
                    genetic_code: 11,
                    ..BatchSettings::default()
                },
            },
            SubmissionEdit::SetRows {
                rows: vec![RowDecision {
                    record: "REF_rbcL".into(),
                    decision: Decision::Existing {
                        accession: "AB123456.1".into(),
                    },
                }],
            },
            // HV101 and HV102 keep their detected frames (HV102's is 3
            // after trimming two Ns). HV103's frameshift has no clean frame;
            // forcing frame 2 leaves internal stops.
            frames(&["HV101_rbcL"], 2),
            frames(&["HV102_rbcL"], 3),
            frames(&["HV103_rbcL"], 2),
        ],
    );
    let coi = validate(
        &root,
        &registry,
        "COI",
        "COI.afa",
        vec![
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
            SubmissionEdit::SetRows {
                rows: vec![RowDecision {
                    record: "HV204_COI".into(),
                    decision: Decision::Exclude,
                }],
            },
            frames(&["HV201_COI", "HV202_COI", "HV203_COI"], 2),
        ],
    );
    let ssu = validate(
        &root,
        &registry,
        "18S",
        "18S.afa",
        vec![SubmissionEdit::SetBatch {
            batch: BatchSettings {
                marker: Marker::Rrna {
                    product: "18S ribosomal RNA".into(),
                },
                ..BatchSettings::default()
            },
        }],
    );
    let its = validate(&root, &registry, "ITS", "ITS.afa", its_edits());
    if let Some(evidence) = std::env::var_os("BASECRAFT_TEST_EVIDENCE") {
        let evidence = Path::new(&evidence);
        for name in ["rbcL", "COI", "18S", "ITS"] {
            copy_tree(&root.join(name), &evidence.join(name));
        }
    }
    let has = |s: &ValidationSummary, code: &str, record: &str| {
        s.issues
            .iter()
            .any(|i| i.code == code && i.record.as_deref() == Some(record))
    };
    for summary in [&rbcl, &coi, &ssu, &its] {
        assert_ne!(summary.status, "unavailable", "{summary:#?}");
        assert_ne!(summary.status, "failed", "{summary:#?}");
    }
    assert!(
        has(&rbcl, "SEQ_FEAT.InternalStop", "HV103_rbcL"),
        "{rbcl:#?}"
    );
    assert!(
        has(&its, "SEQ_DESCR.BadGeoLocNameCode", "HV404_ITS"),
        "{its:#?}"
    );
    // Records without intentional errors pass table2asn.
    for summary in [&coi, &ssu] {
        assert_eq!(summary.errors, 0, "{summary:#?}");
    }
    let clean = |s: &ValidationSummary, records: &[&str]| {
        s.issues.iter().all(|i| {
            i.severity != basecraft_docs::submission::Severity::Blocking
                || !records.contains(&i.record.as_deref().unwrap_or(""))
        })
    };
    assert!(clean(&rbcl, &["HV101_rbcL", "HV102_rbcL"]), "{rbcl:#?}");
    assert!(clean(&its, &["HV401_ITS", "HV402_ITS"]), "{its:#?}");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
