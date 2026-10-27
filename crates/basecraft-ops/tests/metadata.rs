//! Project specimen metadata: storage, import plans, typed edits and replay.
use std::collections::BTreeMap;

use basecraft_ops::metadata::{self, ImportOptions};
use basecraft_ops::{Session, replay};
use basecraft_protocol::Op;
use basecraft_protocol::metadata::{
    Cell, ChangeSet, FieldTarget, HeaderPattern, MetadataEdit, MissingReason, PartialDate,
};

const FASTA: &str = ">HV1|Caulerpa_taxifolia|2019-05\nACGT\n>HV2|Caulerpa_sp._3|not collected\nACGA\n>|orphan\nACGG\n";

fn pattern() -> HeaderPattern {
    HeaderPattern {
        name: "id|taxon|date".into(),
        separator: "|".into(),
        fields: vec![
            FieldTarget::Specimen,
            FieldTarget::Taxon,
            FieldTarget::Column {
                column: "eventDate".into(),
            },
        ],
    }
}

/// Record IDs are the first word; the pattern reads the whole header line.
fn headers(root: &std::path::Path) -> Vec<(String, String)> {
    metadata::fasta_headers(&root.join("rbcL.fasta")).unwrap()
}

#[test]
fn opening_a_project_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::unlogged(dir.path());
    assert!(metadata::open(&mut s, false).unwrap().is_none());
    assert!(!dir.path().join("metadata").exists());
}

#[test]
fn header_import_links_identifies_saves_and_replays() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("rbcL.fasta"), FASTA).unwrap();
    {
        let mut s = Session::new(root);
        let doc = metadata::open(&mut s, true).unwrap().unwrap();
        assert!(s.is_metadata(doc));
        let plan = metadata::plan_headers(
            s.metadata(doc),
            "rbcL.fasta",
            &headers(root),
            &pattern(),
            ImportOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.changes.specimens, ["HV1", "HV2"]);
        assert_eq!(plan.changes.links.len(), 2);
        assert_eq!(plan.changes.taxa.len(), 2);
        assert!(
            plan.issues
                .iter()
                .any(|i| i.kind == "unmatched" && i.message.contains("orphan"))
        );
        metadata::apply_plan(&mut s, doc, plan.changes).unwrap();
        let meta = s.metadata(doc);
        assert_eq!(
            meta.specimens.rows[1].cells["eventDate"],
            Cell::Missing(MissingReason::NotCollected)
        );
        assert_eq!(meta.taxa[1].name, "Caulerpa sp. 3");
        assert_eq!(
            basecraft_docs::metadata::unlinked(
                meta,
                "rbcL.fasta",
                &["HV1|Caulerpa_taxifolia|2019-05".into(), "|orphan".into()]
            ),
            ["|orphan"]
        );
        // Remember the pattern with the project.
        let mut settings = meta.import.clone();
        settings.header_patterns.push(pattern());
        s.apply(
            doc,
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::SetImportSettings { settings }),
            },
        )
        .unwrap();
        // The same import again changes nothing.
        let again = metadata::plan_headers(
            s.metadata(doc),
            "rbcL.fasta",
            &headers(root),
            &pattern(),
            ImportOptions::default(),
        )
        .unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        // A typed edit, undone and redone.
        let change =
            metadata::cell_change(s.metadata(doc), "HV1", "eventDate", "2019-05-12").unwrap();
        s.apply(
            doc,
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::Apply {
                    changes: ChangeSet {
                        cells: vec![change],
                        ..ChangeSet::default()
                    },
                }),
            },
        )
        .unwrap();
        s.undo(doc).unwrap();
        assert_eq!(
            metadata::cell_text(s.metadata(doc), "HV1", "eventDate"),
            "2019-05"
        );
        s.redo(doc).unwrap();
        assert_eq!(
            metadata::cell_text(s.metadata(doc), "HV1", "eventDate"),
            "2019-05-12"
        );
        // A refused edit leaves the document and history alone.
        assert!(
            s.apply(
                doc,
                Op::MetadataEdit {
                    edit: Box::new(MetadataEdit::RemoveSpecimens {
                        specimens: ["HV1".to_string()].into(),
                    }),
                },
            )
            .is_err()
        );
        s.save(doc, None).unwrap();
    }
    let specimens = std::fs::read_to_string(root.join("metadata/specimens.tsv")).unwrap();
    assert!(
        specimens.starts_with("specimen_id\teventDate\n"),
        "{specimens}"
    );
    assert!(specimens.contains("HV2\tnot collected\n"), "{specimens}");
    let records = std::fs::read_to_string(root.join("metadata/records.tsv")).unwrap();
    assert!(
        records.contains("rbcL.fasta\tHV2|Caulerpa_sp._3|not\tHV2\n"),
        "{records}"
    );
    let mut s = Session::unlogged(root);
    let doc = metadata::open(&mut s, false).unwrap().unwrap();
    assert_eq!(s.metadata_notices(doc), Vec::<String>::new());
    let meta = s.metadata(doc);
    assert_eq!(
        meta.specimens.rows[0].cells["eventDate"],
        Cell::Date(PartialDate::parse("2019-05-12").unwrap())
    );
    assert_eq!(meta.import.header_patterns, [pattern()]);
    assert_eq!(meta.identifications.len(), 2);
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    let history = s.document_record(doc).unwrap().report().unwrap();
    assert!(
        serde_json::to_string(&history)
            .unwrap()
            .contains("Specimen metadata")
    );
}

#[test]
fn spreadsheet_import_flags_conflicts_duplicates_and_bad_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("rbcL.fasta"), FASTA).unwrap();
    let mut s = Session::unlogged(root);
    let doc = metadata::open(&mut s, true).unwrap().unwrap();
    let first = metadata::plan_headers(
        s.metadata(doc),
        "rbcL.fasta",
        &headers(root),
        &pattern(),
        ImportOptions::default(),
    )
    .unwrap();
    metadata::apply_plan(&mut s, doc, first.changes).unwrap();
    std::fs::write(
        root.join("sheet.csv"),
        "specimen_id;eventDate;Country;decimalLatitude;notes\nHV1;2018;Australia;-38.1;x\nHV1;2017;Australia;0;y\nHV3;bad date;Fiji;95;z\n",
    )
    .unwrap();
    let sheet = metadata::read_sheet(&root.join("sheet.csv")).unwrap();
    let mapping = metadata::guess_mapping(s.metadata(doc), &sheet.headers);
    assert_eq!(mapping["specimen_id"], FieldTarget::Specimen);
    assert_eq!(
        mapping["Country"],
        FieldTarget::Column {
            column: "country".into()
        }
    );
    assert_eq!(mapping["notes"], FieldTarget::Ignore);
    let plan = metadata::plan_sheet(
        s.metadata(doc),
        &sheet,
        &mapping,
        None,
        ImportOptions {
            create_specimens: false,
            overwrite: false,
        },
    )
    .unwrap();
    let kinds: Vec<&str> = plan.issues.iter().map(|i| i.kind.as_str()).collect();
    assert!(kinds.contains(&"conflict"), "{kinds:?}");
    assert!(kinds.contains(&"duplicate"), "{kinds:?}");
    assert!(kinds.contains(&"unmatched"), "{kinds:?}");
    // The conflicting date is kept; new columns and values come in.
    assert!(!plan.changes.cells.iter().any(|c| c.column == "eventDate"));
    assert!(plan.changes.columns.iter().any(|c| c.name == "country"));
    assert_eq!(plan.changes.cells.len(), 2);
    let overwrite = metadata::plan_sheet(
        s.metadata(doc),
        &sheet,
        &mapping,
        None,
        ImportOptions {
            create_specimens: true,
            overwrite: true,
        },
    )
    .unwrap();
    assert!(
        overwrite
            .changes
            .cells
            .iter()
            .any(|c| c.column == "eventDate")
    );
    assert!(overwrite.issues.iter().any(|i| i.kind == "invalid"));
    metadata::apply_plan(&mut s, doc, overwrite.changes).unwrap();
    assert_eq!(
        metadata::cell_text(s.metadata(doc), "HV1", "eventDate"),
        "2018"
    );
    assert!(s.metadata(doc).specimens.rows.iter().any(|r| r.id == "HV3"));
}

#[test]
fn template_round_trips_through_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("rbcL.fasta"), FASTA).unwrap();
    let mut s = Session::unlogged(root);
    let doc = metadata::open(&mut s, true).unwrap().unwrap();
    let records: Vec<(String, String)> = headers(root)
        .into_iter()
        .map(|(id, _)| ("rbcL.fasta".to_string(), id))
        .collect();
    let path = root.join("template.ods");
    metadata::write_template(s.metadata(doc), &records, &path).unwrap();
    let mut sheet = metadata::read_sheet(&path).unwrap();
    assert_eq!(
        &sheet.headers[..4],
        ["document", "record", "specimen_id", "organism"]
    );
    assert_eq!(sheet.rows.len(), 3);
    // Someone fills it in.
    let column = |name: &str| sheet.column(name).unwrap();
    let (id, organism, date) = (
        column("specimen_id"),
        column("organism"),
        column("eventDate"),
    );
    for (row, specimen) in ["A1", "A1", "A2"].iter().enumerate() {
        sheet.rows[row][id] = (*specimen).into();
        sheet.rows[row][organism] = "Codium fragile".into();
        sheet.rows[row][date] = "2020-01".into();
    }
    let mapping = metadata::guess_mapping(s.metadata(doc), &sheet.headers);
    let plan = metadata::plan_sheet(
        s.metadata(doc),
        &sheet,
        &mapping,
        None,
        ImportOptions::default(),
    )
    .unwrap();
    // A1 appears twice: its second row is a duplicate and its record isn't linked.
    assert_eq!(plan.changes.specimens, ["A1", "A2"]);
    assert_eq!(plan.changes.links.len(), 2);
    assert!(plan.issues.iter().any(|i| i.kind == "duplicate"));
    metadata::apply_plan(&mut s, doc, plan.changes).unwrap();
    let source = basecraft_docs::metadata::insdc_source(s.metadata(doc), "A1").unwrap();
    let map: BTreeMap<_, _> = source.qualifiers.into_iter().collect();
    assert_eq!(map["organism"], "Codium fragile");
    assert_eq!(map["collection_date"], "2020-01");
}

#[test]
fn outside_edits_future_versions_and_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = metadata::create(root).unwrap();
    assert!(metadata::create(root).is_err());
    // A later Basecraft's extra fields survive a save.
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    value["later_feature"] = serde_json::json!({"kept": true});
    std::fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    // An outside edit to a table is read, checked and reported.
    std::fs::write(root.join("metadata/specimens.tsv"), "specimen_id\nHV9\n").unwrap();
    {
        let mut s = Session::unlogged(root);
        let doc = s.open(&manifest).unwrap();
        assert_eq!(s.metadata_notices(doc).len(), 1);
        assert_eq!(s.metadata(doc).specimens.rows[0].id, "HV9");
        s.apply(
            doc,
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::Apply {
                    changes: ChangeSet {
                        specimens: vec!["HV10".into()],
                        ..ChangeSet::default()
                    },
                }),
            },
        )
        .unwrap();
        s.save(doc, None).unwrap();
    }
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(saved["later_feature"]["kept"], true);
    let mut s = Session::unlogged(root);
    let doc = s.open(&manifest).unwrap();
    assert_eq!(s.metadata_notices(doc), Vec::<String>::new());
    drop(s);
    // An undeclared column or a bad value is refused with its location.
    std::fs::write(
        root.join("metadata/specimens.tsv"),
        "specimen_id\tcolour\nHV9\tred\n",
    )
    .unwrap();
    let error = Session::unlogged(root)
        .open(&manifest)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("import the edited table"), "{error}");
    // A newer format is refused clearly.
    let mut newer = saved.clone();
    newer["version"] = serde_json::json!(2);
    std::fs::write(&manifest, serde_json::to_vec(&newer).unwrap()).unwrap();
    let error = Session::unlogged(root)
        .open(&manifest)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("newer Basecraft"), "{error}");
}
