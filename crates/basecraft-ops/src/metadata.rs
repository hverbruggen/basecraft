//! Project specimen metadata: storage as TSV tables plus a versioned JSON
//! manifest, import planning (FASTA headers and spreadsheets) into
//! reviewable change sets, and the `.ods` template.
//!
//! Every import route only *plans*: it returns a [`Plan`] holding one
//! [`ChangeSet`] and the issues found. Callers show it for review and apply
//! it as one `metadata.edit` operation, so GUI, CLI, Python and MCP edits
//! share undo, replay and history.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use basecraft_docs::metadata::{
    self as model, SPECIMEN_ID, current_identification, format_cell, parse_cell,
};
use basecraft_protocol::metadata::{
    Cell, CellChange, ChangeSet, Column, FieldTarget, HeaderPattern, Identification,
    METADATA_VERSION, ProjectMetadata, RecordLink, Row, Taxon,
};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::error::{damaged, invalid};

use crate::{Result, Session};
pub use basecraft_docs::metadata::{
    InsdcSource, KNOWN_COLUMNS, KnownColumn, insdc_source, valid_column_name,
};

/// The project folder holding the metadata tables.
pub const DIRECTORY: &str = "metadata";
/// The manifest file name; it identifies a project metadata document.
pub const MANIFEST: &str = "metadata.basecraft.json";
const FORMAT: &str = "basecraft.project-metadata";
const SPECIMENS: &str = "specimens.tsv";
const TAXA: &str = "taxa.tsv";
const IDENTIFICATIONS: &str = "identifications.tsv";
const RECORDS: &str = "records.tsv";
const TAXA_HEADER: [&str; 4] = [
    "taxon_id",
    "scientific_name",
    "submitted_name",
    "ncbi_taxid",
];
const IDENTIFICATIONS_HEADER: [&str; 6] = [
    "identification_id",
    SPECIMEN_ID,
    "taxon_id",
    "identified_by",
    "date_identified",
    "remarks",
];
const RECORDS_HEADER: [&str; 3] = ["document", "record", SPECIMEN_ID];

/// The project's metadata manifest path.
pub fn manifest_path(project: &Path) -> PathBuf {
    project.join(DIRECTORY).join(MANIFEST)
}

/// Whether `path` names a metadata manifest.
pub fn is_manifest(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == MANIFEST)
}

/// The files a metadata document consists of, manifest first.
pub fn files(manifest: &Path) -> Vec<PathBuf> {
    let dir = manifest.parent().unwrap_or(Path::new("."));
    std::iter::once(manifest.to_path_buf())
        .chain(
            [SPECIMENS, TAXA, IDENTIFICATIONS, RECORDS]
                .into_iter()
                .map(|f| dir.join(f)),
        )
        .collect()
}

use basecraft_protocol::metadata_records::{MetadataManifest as Manifest, TableEntry, Tables};

/// A metadata document as opened: its contents and anything noticed while
/// reading it.
#[derive(Clone, Debug)]
pub struct OpenedMetadata {
    pub metadata: ProjectMetadata,
    /// Tables edited outside Basecraft since its last save. They were read
    /// and validated against the declared column types.
    pub notices: Vec<String>,
}

fn optional(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

/// Read a metadata document from its manifest.
pub fn load(manifest_path: &Path) -> Result<OpenedMetadata> {
    load_with(manifest_path, &mut |path| {
        Ok(basecraft_io::Source::open_hashed(path)?)
    })
}

pub(crate) fn load_with(
    manifest_path: &Path,
    read: &mut impl FnMut(&Path) -> Result<(basecraft_io::Source, String)>,
) -> Result<OpenedMetadata> {
    let dir = manifest_path.parent().unwrap_or(Path::new("."));
    let (value, writer) = basecraft_protocol::records::decode_written::<serde_json::Value>(
        read(manifest_path)?.0.bytes(),
    )
    .map_err(|e| Error::Damaged {
        path: manifest_path.to_path_buf(),
        detail: e.to_string(),
    })?;
    if value["format"] != FORMAT {
        return Err(Error::Damaged {
            path: manifest_path.to_path_buf(),
            detail: format!("{MANIFEST} is not a Basecraft metadata manifest"),
        });
    }
    let version = value["version"].as_u64().unwrap_or(0);
    if version != u64::from(METADATA_VERSION) {
        return Err(Error::Unsupported(
            basecraft_protocol::records::other_format(&writer, version, METADATA_VERSION),
        ));
    }
    let manifest: Manifest = basecraft_protocol::records::convert(value, &writer).map_err(|e| {
        Error::Damaged {
            path: manifest_path.to_path_buf(),
            detail: format!("{MANIFEST} is malformed: {e}"),
        }
    })?;
    let mut notices = Vec::new();
    let mut read = |entry: &TableEntry, expected: &str| -> Result<basecraft_io::table::Sheet> {
        if entry.file != expected {
            return Err(Error::Damaged {
                path: manifest_path.to_path_buf(),
                detail: format!(
                    "{MANIFEST} names table {} where {expected} was expected",
                    entry.file
                ),
            });
        }
        let (source, hash) = read(&dir.join(expected))?;
        let bytes = source.bytes();
        if hash != entry.hash {
            notices.push(format!(
                "{expected} was changed outside Basecraft since the last save; it was read and checked against its column types"
            ));
        }
        basecraft_io::table::read_tsv(bytes).map_err(|e| Error::Damaged {
            path: dir.join(expected),
            detail: format!("{expected}: {e}"),
        })
    };
    let tables = &manifest.tables;
    let specimens = read(&tables.specimens, SPECIMENS)?;
    let taxa = read(&tables.taxa, TAXA)?;
    let identifications = read(&tables.identifications, IDENTIFICATIONS)?;
    let records = read(&tables.records, RECORDS)?;
    let columns = tables.specimens.columns.clone();
    let expected: Vec<&str> = std::iter::once(SPECIMEN_ID)
        .chain(columns.iter().map(|c| c.name.as_str()))
        .collect();
    if specimens.headers != expected {
        return Err(Error::Damaged {
            path: dir.join(SPECIMENS),
            detail: format!(
                "the columns of {SPECIMENS} differ from those declared in {MANIFEST}; import the edited table as a spreadsheet instead"
            ),
        });
    }
    let mut rows = Vec::with_capacity(specimens.rows.len());
    for (line, fields) in specimens.rows.iter().enumerate() {
        let mut cells = BTreeMap::new();
        for (column, text) in columns.iter().zip(&fields[1..]) {
            let cell = parse_cell(column, text).map_err(|e| Error::Damaged {
                path: dir.join(SPECIMENS),
                detail: format!("{SPECIMENS} line {}: {e}", line + 2),
            })?;
            if let Some(cell) = cell {
                cells.insert(column.name.clone(), cell);
            }
        }
        rows.push(Row {
            id: fields[0].clone(),
            cells,
        });
    }
    let check = |sheet: &basecraft_io::table::Sheet, header: &[&str], file: &str| {
        if sheet.headers != header {
            return Err(Error::Damaged {
                path: dir.join(file),
                detail: format!("{file} has unexpected columns"),
            });
        }
        Ok(())
    };
    check(&taxa, &TAXA_HEADER, TAXA)?;
    check(&identifications, &IDENTIFICATIONS_HEADER, IDENTIFICATIONS)?;
    check(&records, &RECORDS_HEADER, RECORDS)?;
    let taxa = taxa
        .rows
        .iter()
        .enumerate()
        .map(|(line, f)| {
            Ok(Taxon {
                id: f[0].clone(),
                name: f[1].clone(),
                submitted_name: optional(&f[2]),
                taxid: optional(&f[3])
                    .map(|t| {
                        t.parse().map_err(|_| Error::Damaged {
                            path: dir.join(TAXA),
                            detail: format!("{TAXA} line {}: {t:?} is not a TaxID", line + 2),
                        })
                    })
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let identifications = identifications
        .rows
        .iter()
        .enumerate()
        .map(|(line, f)| {
            Ok(Identification {
                id: f[0].clone(),
                specimen: f[1].clone(),
                taxon: f[2].clone(),
                identified_by: optional(&f[3]),
                date: optional(&f[4])
                    .map(|d| {
                        basecraft_protocol::metadata::PartialDate::parse(&d).map_err(|e| {
                            Error::Damaged {
                                path: dir.join(IDENTIFICATIONS),
                                detail: format!("{IDENTIFICATIONS} line {}: {e}", line + 2),
                            }
                        })
                    })
                    .transpose()?,
                remarks: optional(&f[5]),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let links = records
        .rows
        .iter()
        .map(|f| RecordLink {
            document: f[0].clone(),
            record: f[1].clone(),
            specimen: f[2].clone(),
        })
        .collect();
    let metadata = ProjectMetadata {
        version: manifest.version,
        specimens: basecraft_protocol::metadata::Table { columns, rows },
        taxa,
        identifications,
        links,
        import: manifest.import,
        partitions: manifest.partitions,
        extra: manifest.extra,
    };
    model::validate(&metadata).map_err(|e| damaged(manifest_path, e.0))?;
    Ok(OpenedMetadata { metadata, notices })
}

/// The bytes of every file, in the order of [`files`]: manifest first.
/// Tables are written before the manifest, which records their hashes.
pub fn encode(meta: &ProjectMetadata) -> Result<Vec<Vec<u8>>> {
    let tsv = |header: &[&str], rows: &[Vec<String>]| -> Result<Vec<u8>> {
        let mut out = Vec::new();
        basecraft_io::table::write_tsv(&mut out, header, rows)?;
        Ok(out)
    };
    let header: Vec<&str> = std::iter::once(SPECIMEN_ID)
        .chain(meta.specimens.columns.iter().map(|c| c.name.as_str()))
        .collect();
    let specimens = tsv(
        &header,
        &meta
            .specimens
            .rows
            .iter()
            .map(|row| {
                std::iter::once(row.id.clone())
                    .chain(
                        meta.specimens
                            .columns
                            .iter()
                            .map(|c| row.cells.get(&c.name).map(format_cell).unwrap_or_default()),
                    )
                    .collect()
            })
            .collect::<Vec<_>>(),
    )?;
    let taxa = tsv(
        &TAXA_HEADER,
        &meta
            .taxa
            .iter()
            .map(|t| {
                vec![
                    t.id.clone(),
                    t.name.clone(),
                    t.submitted_name.clone().unwrap_or_default(),
                    t.taxid.map(|n| n.to_string()).unwrap_or_default(),
                ]
            })
            .collect::<Vec<_>>(),
    )?;
    let identifications = tsv(
        &IDENTIFICATIONS_HEADER,
        &meta
            .identifications
            .iter()
            .map(|i| {
                vec![
                    i.id.clone(),
                    i.specimen.clone(),
                    i.taxon.clone(),
                    i.identified_by.clone().unwrap_or_default(),
                    i.date.map(|d| d.to_string()).unwrap_or_default(),
                    i.remarks.clone().unwrap_or_default(),
                ]
            })
            .collect::<Vec<_>>(),
    )?;
    let records = tsv(
        &RECORDS_HEADER,
        &meta
            .links
            .iter()
            .map(|l| vec![l.document.clone(), l.record.clone(), l.specimen.clone()])
            .collect::<Vec<_>>(),
    )?;
    let entry = |file: &str, bytes: &[u8], columns: Vec<Column>| TableEntry {
        file: file.into(),
        hash: basecraft_io::hash_bytes(bytes),
        columns,
    };
    let manifest = Manifest {
        format: FORMAT.into(),
        version: meta.version,
        tables: Tables {
            specimens: entry(SPECIMENS, &specimens, meta.specimens.columns.clone()),
            taxa: entry(TAXA, &taxa, Vec::new()),
            identifications: entry(IDENTIFICATIONS, &identifications, Vec::new()),
            records: entry(RECORDS, &records, Vec::new()),
        },
        import: meta.import.clone(),
        partitions: meta.partitions.clone(),
        extra: meta.extra.clone(),
    };
    let manifest = basecraft_protocol::records::encode(&manifest)?;
    Ok(vec![manifest, specimens, taxa, identifications, records])
}

/// Write an empty metadata document for `project` and return its manifest
/// path. Refuses to replace existing metadata. This is the only route that
/// creates the files; viewing a project never does.
pub fn create(project: &Path) -> Result<PathBuf> {
    let manifest = manifest_path(project);
    let paths = files(&manifest);
    if paths.iter().any(|p| p.exists()) {
        return Err(Error::OutputConflict(format!(
            "{} already has metadata files; open them instead",
            project.join(DIRECTORY).display()
        )));
    }
    std::fs::create_dir_all(project.join(DIRECTORY))?;
    let encoded = encode(&ProjectMetadata::default())?;
    // Tables first, manifest last: a manifest always names complete tables.
    for (path, bytes) in paths.iter().zip(&encoded).rev() {
        basecraft_io::write_atomic(path, |w| w.write_all(bytes))?;
    }
    Ok(manifest)
}

/// Open the project's metadata in `session`, creating it first when
/// `create_missing` is set. Without metadata files and without
/// `create_missing`, returns `None`.
pub fn open(session: &mut Session, create_missing: bool) -> Result<Option<crate::DocId>> {
    if let Some(doc) = session.project_metadata_id() {
        return Ok(Some(doc));
    }
    let manifest = manifest_path(session.root());
    if !manifest.is_file() {
        if !create_missing {
            return Ok(None);
        }
        create(session.root())?;
    }
    session.open(&manifest).map(Some)
}

// Import planning.

/// Something an import found that the reviewer should see.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// `duplicate`, `conflict`, `unmatched`, `invalid`, `imprecise_date` or
    /// `ignored`.
    pub kind: String,
    /// One-based source row (after the header), when the issue has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    pub message: String,
    /// Whether the change is part of the plan despite the issue.
    pub included: bool,
}

/// A reviewable import: the change set to apply and what was found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub changes: ChangeSet,
    pub issues: Vec<Issue>,
    /// Source rows read.
    pub rows: usize,
    /// Rows that matched an existing specimen.
    pub matched: usize,
    /// The spreadsheet mapping used, for review and to remember.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mapping: BTreeMap<String, FieldTarget>,
}

/// How an import treats existing data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Add specimens the roster lacks. Off: such rows are unmatched.
    #[serde(default = "yes")]
    pub create_specimens: bool,
    /// Replace existing values that differ. Off: they stay, flagged.
    #[serde(default)]
    pub overwrite: bool,
}

fn yes() -> bool {
    true
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            create_specimens: true,
            overwrite: false,
        }
    }
}

/// One source row, already split into fields by target.
struct SourceRow {
    row: usize,
    specimen: String,
    records: Vec<String>,
    taxon: Option<String>,
    values: Vec<(String, String)>,
}

struct Planner<'a> {
    meta: &'a ProjectMetadata,
    options: ImportOptions,
    document: Option<String>,
    plan: Plan,
    new_specimens: BTreeSet<String>,
    seen: BTreeSet<String>,
    taxa_by_name: BTreeMap<String, String>,
    identified: BTreeMap<String, String>,
    linked: BTreeSet<(String, String)>,
}

impl<'a> Planner<'a> {
    fn new(meta: &'a ProjectMetadata, options: ImportOptions, document: Option<String>) -> Self {
        Self {
            meta,
            options,
            document,
            plan: Plan::default(),
            new_specimens: BTreeSet::new(),
            seen: BTreeSet::new(),
            taxa_by_name: meta
                .taxa
                .iter()
                .map(|t| (t.name.clone(), t.id.clone()))
                .collect(),
            identified: BTreeMap::new(),
            linked: BTreeSet::new(),
        }
    }

    fn issue(&mut self, kind: &str, row: Option<usize>, message: String, included: bool) {
        self.plan.issues.push(Issue {
            kind: kind.into(),
            row,
            message,
            included,
        });
    }

    fn column(&self, name: &str) -> Option<&Column> {
        self.meta
            .specimens
            .columns
            .iter()
            .chain(&self.plan.changes.columns)
            .find(|c| c.name == name)
    }

    /// Declare known Darwin Core columns the import uses but the table lacks.
    fn ensure_column(&mut self, name: &str) -> bool {
        if self.column(name).is_some() {
            return true;
        }
        if let Some(known) = model::known_column(name) {
            self.plan.changes.columns.push(known.column());
            return true;
        }
        false
    }

    fn add(&mut self, source: SourceRow) {
        let row = Some(source.row);
        self.plan.rows += 1;
        if !model::valid_id(&source.specimen) {
            self.issue(
                "invalid",
                row,
                format!(
                    "{:?} is not a usable specimen ID; row skipped",
                    source.specimen
                ),
                false,
            );
            return;
        }
        let specimen = source.specimen;
        if !self.seen.insert(specimen.clone()) {
            self.issue(
                "duplicate",
                row,
                format!("specimen {specimen} appears again; this row was skipped"),
                false,
            );
            return;
        }
        let existing = self.meta.specimens.rows.iter().find(|r| r.id == specimen);
        if existing.is_some() {
            self.plan.matched += 1;
        } else if self.options.create_specimens {
            self.new_specimens.insert(specimen.clone());
            self.plan.changes.specimens.push(specimen.clone());
        } else {
            self.issue(
                "unmatched",
                row,
                format!("specimen {specimen} is not in the roster; row skipped"),
                false,
            );
            return;
        }
        for (name, text) in source.values {
            if text.trim().is_empty() {
                continue;
            }
            if !self.ensure_column(&name) {
                self.issue(
                    "invalid",
                    row,
                    format!("there is no column {name}; add it before importing"),
                    false,
                );
                continue;
            }
            let Some(column) = self.column(&name).cloned() else {
                continue;
            };
            let value = match parse_cell(&column, &text) {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(e) => {
                    self.issue("invalid", row, format!("{specimen} {name}: {e}"), false);
                    continue;
                }
            };
            let current = existing.and_then(|r| r.cells.get(&name));
            if current == Some(&value) {
                continue;
            }
            if let Some(current) = current {
                let included = self.options.overwrite;
                self.issue(
                    "conflict",
                    row,
                    format!(
                        "{specimen} {name} is {:?}; the import has {:?}{}",
                        format_cell(current),
                        format_cell(&value),
                        if included {
                            " (replacing it)"
                        } else {
                            " (kept)"
                        }
                    ),
                    included,
                );
                if !included {
                    continue;
                }
            }
            self.plan.changes.cells.push(CellChange {
                specimen: specimen.clone(),
                column: name,
                value: Some(value),
            });
        }
        if let Some(name) = source.taxon.filter(|n| !n.trim().is_empty()) {
            self.identify(&specimen, name.trim(), row);
        }
        for record in source.records {
            self.link(&specimen, &record, row);
        }
    }

    fn identify(&mut self, specimen: &str, name: &str, row: Option<usize>) {
        if !model::valid_id(name) {
            self.issue(
                "invalid",
                row,
                format!("{name:?} is not a usable taxon name"),
                false,
            );
            return;
        }
        let taxon = if let Some(id) = self.taxa_by_name.get(name) {
            id.clone()
        } else {
            let id = ulid::Ulid::generate().to_string();
            self.taxa_by_name.insert(name.into(), id.clone());
            self.plan.changes.taxa.push(Taxon {
                id: id.clone(),
                name: name.into(),
                submitted_name: None,
                taxid: None,
            });
            id
        };
        let current = self
            .identified
            .get(specimen)
            .cloned()
            .or_else(|| current_identification(self.meta, specimen).map(|i| i.taxon.clone()));
        if current.as_deref() == Some(taxon.as_str()) {
            return;
        }
        if let Some(previous) = current {
            let previous =
                model::taxon(self.meta, &previous).map_or(previous.clone(), |t| t.name.clone());
            self.issue(
                "conflict",
                row,
                format!(
                    "{specimen} is identified as {previous}; the import re-identifies it as {name}"
                ),
                true,
            );
        }
        self.identified.insert(specimen.into(), taxon.clone());
        self.plan.changes.identifications.push(Identification {
            id: ulid::Ulid::generate().to_string(),
            specimen: specimen.into(),
            taxon,
            identified_by: None,
            date: None,
            remarks: None,
        });
    }

    fn link(&mut self, specimen: &str, record: &str, row: Option<usize>) {
        let record = record.trim();
        if record.is_empty() {
            return;
        }
        let Some(document) = self.document.clone() else {
            self.issue(
                "unmatched",
                row,
                format!("record {record} not linked: choose the sequence file it belongs to"),
                false,
            );
            return;
        };
        if !self.linked.insert((document.clone(), record.into())) {
            self.issue(
                "duplicate",
                row,
                format!("record {record} is linked twice in this import; the first link was kept"),
                false,
            );
            return;
        }
        match model::linked_specimen(self.meta, &document, record) {
            Some(current) if current == specimen => return,
            Some(current) => {
                let included = self.options.overwrite;
                self.issue(
                    "conflict",
                    row,
                    format!(
                        "record {record} is linked to {current}; the import links it to {specimen}{}",
                        if included { " (relinking)" } else { " (kept)" }
                    ),
                    included,
                );
                if !included {
                    return;
                }
            }
            None => {}
        }
        self.plan.changes.links.push(RecordLink {
            document,
            record: record.into(),
            specimen: specimen.into(),
        });
    }
}

/// The records of a FASTA file: ID (up to the first whitespace) and full
/// header line.
pub fn fasta_headers(path: &Path) -> Result<Vec<(String, String)>> {
    let file = basecraft_io::fasta::IndexedFasta::open(path)?;
    Ok(file
        .index
        .records
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                String::from_utf8_lossy(r.header(file.bytes())).into_owned(),
            )
        })
        .collect())
}

/// Plan an import from structured FASTA headers. `document` is the
/// project-relative path of the FASTA file; every record is linked to the
/// specimen its header names. Records whose header has no specimen field
/// are reported as unmatched.
pub fn plan_headers(
    meta: &ProjectMetadata,
    document: &str,
    headers: &[(String, String)],
    pattern: &HeaderPattern,
    options: ImportOptions,
) -> Result<Plan> {
    let choose = |what: &str| Error::MissingChoice(what.into());
    if pattern.separator.is_empty() {
        return Err(choose("choose a field separator"));
    }
    if !pattern.fields.contains(&FieldTarget::Specimen) {
        return Err(choose("choose which header field holds the specimen ID"));
    }
    let mut planner = Planner::new(meta, options, Some(document.into()));
    for (index, (record, header)) in headers.iter().enumerate() {
        let fields = basecraft_docs::metadata::header_fields(header, &pattern.separator)
            .ok_or_else(|| choose("choose a field separator"))?;
        let mut source = SourceRow {
            row: index + 1,
            specimen: String::new(),
            records: vec![record.clone()],
            taxon: None,
            values: Vec::new(),
        };
        for (target, field) in pattern.fields.iter().zip(&fields) {
            let field = field.trim();
            match target {
                FieldTarget::Specimen => source.specimen = field.into(),
                FieldTarget::Taxon => source.taxon = Some(field.replace('_', " ")),
                FieldTarget::Column { column } => {
                    source.values.push((column.clone(), field.into()));
                }
                FieldTarget::Record | FieldTarget::Ignore => {}
            }
        }
        if source.specimen.is_empty() {
            planner.issue(
                "unmatched",
                Some(index + 1),
                format!("record {record}: its header has no specimen field"),
                false,
            );
            planner.plan.rows += 1;
            continue;
        }
        planner.add(source);
    }
    Ok(planner.plan)
}

/// Guess a spreadsheet mapping from column headers: `specimen_id`,
/// `record`, `organism`/`scientificName` and known or existing column names.
pub fn guess_mapping(meta: &ProjectMetadata, headers: &[String]) -> BTreeMap<String, FieldTarget> {
    headers
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| {
            let lower = h.to_ascii_lowercase();
            let target = match lower.as_str() {
                "specimen_id" | "specimen" | "specimenid" => FieldTarget::Specimen,
                "record" | "record_id" | "sequence_id" | "sequence" => FieldTarget::Record,
                "organism" | "scientificname" | "scientific_name" | "taxon" | "species" => {
                    FieldTarget::Taxon
                }
                "document" => FieldTarget::Ignore,
                _ => meta
                    .specimens
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .chain(model::KNOWN_COLUMNS.iter().map(|c| c.name))
                    .find(|n| n.eq_ignore_ascii_case(h))
                    .map_or(FieldTarget::Ignore, |n| FieldTarget::Column {
                        column: n.into(),
                    }),
            };
            (h.clone(), target)
        })
        .collect()
}

/// Plan an import from a spreadsheet. `mapping` maps column headers to
/// targets; exactly one column must be the specimen key. Record IDs are
/// linked in `document` (project-relative), or reported unmatched without
/// one. A template's `document` column is used when `document` is `None`.
pub fn plan_sheet(
    meta: &ProjectMetadata,
    sheet: &basecraft_io::table::Sheet,
    mapping: &BTreeMap<String, FieldTarget>,
    document: Option<&str>,
    options: ImportOptions,
) -> Result<Plan> {
    let keys: Vec<&String> = mapping
        .iter()
        .filter(|(_, t)| **t == FieldTarget::Specimen)
        .map(|(h, _)| h)
        .collect();
    let [key] = keys.as_slice() else {
        return Err(Error::MissingChoice(
            "map exactly one column to the specimen ID".into(),
        ));
    };
    let key = sheet
        .column(key)
        .ok_or_else(|| invalid(format!("the spreadsheet has no column {key}")))?;
    let document_column = sheet.column("document");
    let mut planner = Planner::new(meta, options, document.map(str::to_string));
    for (header, target) in mapping {
        if sheet.column(header).is_none() && *target != FieldTarget::Ignore {
            return Err(invalid(format!("the spreadsheet has no column {header}")));
        }
    }
    for header in &sheet.headers {
        if !header.is_empty() && !mapping.contains_key(header) {
            planner.issue(
                "ignored",
                None,
                format!("column {header} is not mapped and was ignored"),
                false,
            );
        }
    }
    for (index, fields) in sheet.rows.iter().enumerate() {
        let mut source = SourceRow {
            row: index + 1,
            specimen: fields[key].trim().to_string(),
            records: Vec::new(),
            taxon: None,
            values: Vec::new(),
        };
        for (column, header) in sheet.headers.iter().enumerate() {
            let text = fields[column].trim();
            match mapping.get(header) {
                Some(FieldTarget::Record) => source.records.push(text.into()),
                Some(FieldTarget::Taxon) => source.taxon = Some(text.into()),
                Some(FieldTarget::Column { column: name }) => {
                    if sheet.date_cells.contains(&(index, column)) && !text.is_empty() {
                        planner.issue(
                            "imprecise_date",
                            Some(index + 1),
                            format!(
                                "{} {name}: the spreadsheet stored {text} as a date, so its precision is a day; type partial dates as text",
                                source.specimen
                            ),
                            true,
                        );
                    }
                    source.values.push((name.clone(), text.into()));
                }
                _ => {}
            }
        }
        if document.is_none() {
            planner.document = document_column
                .map(|c| fields[c].trim().to_string())
                .filter(|d| !d.is_empty());
        }
        if source.specimen.is_empty() {
            planner.issue(
                "unmatched",
                Some(index + 1),
                "row has no specimen ID; skipped".into(),
                false,
            );
            planner.plan.rows += 1;
            continue;
        }
        planner.add(source);
    }
    Ok(planner.plan)
}

/// Read a spreadsheet (`.ods`, `.xlsx`, CSV or TSV) for import.
pub fn read_sheet(path: &Path) -> Result<basecraft_io::table::Sheet> {
    Ok(basecraft_io::table::read_spreadsheet(path)?)
}

/// Rows for the template: one per record, with its document and current
/// link, and the linked specimen's values.
pub fn template_rows(
    meta: &ProjectMetadata,
    records: &[(String, String)],
) -> (Vec<String>, Vec<Vec<String>>) {
    let mut columns: Vec<Column> = model::KNOWN_COLUMNS
        .iter()
        .map(KnownColumn::column)
        .collect();
    for column in &meta.specimens.columns {
        if !columns.iter().any(|c| c.name == column.name) {
            columns.push(column.clone());
        }
    }
    let headers: Vec<String> = ["document", "record", SPECIMEN_ID, "organism"]
        .into_iter()
        .map(str::to_string)
        .chain(columns.iter().map(|c| c.name.clone()))
        .collect();
    let rows = records
        .iter()
        .map(|(document, record)| {
            let specimen = model::linked_specimen(meta, document, record);
            let row = specimen.and_then(|s| meta.specimens.rows.iter().find(|r| r.id == s));
            let organism = specimen
                .and_then(|s| current_identification(meta, s))
                .and_then(|i| model::taxon(meta, &i.taxon))
                .map(|t| t.name.clone())
                .unwrap_or_default();
            [
                document.clone(),
                record.clone(),
                specimen.unwrap_or_default().to_string(),
                organism,
            ]
            .into_iter()
            .chain(columns.iter().map(|c| {
                row.and_then(|r| r.cells.get(&c.name))
                    .map(format_cell)
                    .unwrap_or_default()
            }))
            .collect()
        })
        .collect();
    (headers, rows)
}

/// Write the `.ods` template for `records` ((document, record) pairs) to
/// `path`, with a second sheet explaining the columns. Every cell is text.
pub fn write_template(
    meta: &ProjectMetadata,
    records: &[(String, String)],
    path: &Path,
) -> Result<()> {
    let (headers, rows) = template_rows(meta, records);
    let help_headers = vec!["column".to_string(), "what to enter".into()];
    let mut help: Vec<Vec<String>> = vec![
        vec![
            "document".into(),
            "Sequence file the record is in (leave as written)".into(),
        ],
        vec![
            "record".into(),
            "Sequence record ID (leave as written)".into(),
        ],
        vec![
            SPECIMEN_ID.into(),
            "Your stable ID for the specimen; several records may share one".into(),
        ],
        vec![
            "organism".into(),
            "Taxon name the specimen is identified as".into(),
        ],
    ];
    for name in headers.iter().skip(4) {
        let description = model::known_column(name).map_or_else(
            || {
                meta.specimens
                    .columns
                    .iter()
                    .find(|c| &c.name == name)
                    .map(|c| c.description.clone())
                    .unwrap_or_default()
            },
            |k| {
                let format = match k.kind {
                    model::KnownKind::Date => "; YYYY, YYYY-MM or YYYY-MM-DD",
                    model::KnownKind::Latitude | model::KnownKind::Longitude => {
                        "; decimal degrees, negative for south or west"
                    }
                    model::KnownKind::Text => "",
                };
                format!("{}{format}", k.description)
            },
        );
        help.push(vec![name.clone(), description]);
    }
    help.push(vec![
        "(any column)".into(),
        format!(
            "If a value is unavailable, enter one of: {}",
            basecraft_protocol::metadata::MissingReason::ALL
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ),
    ]);
    let bytes = basecraft_io::table::write_ods(&[
        basecraft_io::table::OdsSheet {
            name: "Specimens",
            headers: &headers,
            rows: &rows,
        },
        basecraft_io::table::OdsSheet {
            name: "Help",
            headers: &help_headers,
            rows: &help,
        },
    ])?;
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    Ok(())
}

/// Apply a reviewed change set as one undoable `metadata.edit`.
pub fn apply_plan(session: &mut Session, doc: crate::DocId, changes: ChangeSet) -> Result<()> {
    if changes.is_empty() {
        return Err(Error::NothingTo("import"));
    }
    session.apply(
        doc,
        basecraft_protocol::Op::MetadataEdit {
            edit: Box::new(basecraft_protocol::metadata::MetadataEdit::Apply { changes }),
        },
    )?;
    Ok(())
}

/// Readable summary of a change set, for reviews and history.
pub fn summary(changes: &ChangeSet) -> String {
    let mut parts = Vec::new();
    let mut count = |n: usize, one: &str, many: &str| {
        if n > 0 {
            parts.push(format!("{n} {}", if n == 1 { one } else { many }));
        }
    };
    count(changes.specimens.len(), "new specimen", "new specimens");
    count(changes.cells.len(), "value", "values");
    count(changes.columns.len(), "new column", "new columns");
    count(changes.taxa.len(), "taxon", "taxa");
    count(
        changes.identifications.len(),
        "identification",
        "identifications",
    );
    count(changes.links.len(), "record link", "record links");
    if parts.is_empty() {
        "no changes".into()
    } else {
        parts.join(", ")
    }
}

/// The value a specimen holds in `column`, as text.
pub fn cell_text(meta: &ProjectMetadata, specimen: &str, column: &str) -> String {
    meta.specimens
        .rows
        .iter()
        .find(|r| r.id == specimen)
        .and_then(|r| r.cells.get(column))
        .map(format_cell)
        .unwrap_or_default()
}

/// Typed change for one cell from text, checked against its column.
pub fn cell_change(
    meta: &ProjectMetadata,
    specimen: &str,
    column: &str,
    text: &str,
) -> Result<CellChange> {
    let declared = meta
        .specimens
        .columns
        .iter()
        .find(|c| c.name == column)
        .ok_or_else(|| invalid(format!("there is no column {column}")))?;
    let value: Option<Cell> = parse_cell(declared, text).map_err(invalid)?;
    Ok(CellChange {
        specimen: specimen.into(),
        column: column.into(),
        value,
    })
}

/// One line for history: what a metadata edit changed.
pub fn describe(edit: &basecraft_protocol::metadata::MetadataEdit) -> String {
    use basecraft_protocol::metadata::MetadataEdit;
    match edit {
        MetadataEdit::PutPartition { partition } => {
            format!("Saved project partition {}", partition.name)
        }
        MetadataEdit::RemovePartition { id } => format!("Removed project partition {id}"),
        MetadataEdit::Apply { changes } => format!("Specimen metadata: {}", summary(changes)),
        MetadataEdit::RemoveSpecimens { specimens } => format!(
            "Removed {} from the specimen roster",
            if specimens.len() == 1 {
                format!(
                    "specimen {}",
                    specimens.iter().next().map_or("", String::as_str)
                )
            } else {
                format!("{} specimens", specimens.len())
            }
        ),
        MetadataEdit::RemoveColumn { column } => format!("Removed specimen column {column}"),
        MetadataEdit::Unlink { records } => format!(
            "Unlinked {} sequence record{} from specimens",
            records.len(),
            if records.len() == 1 { "" } else { "s" }
        ),
        MetadataEdit::RemoveIdentifications { identifications } => format!(
            "Withdrew {} identification{}",
            identifications.len(),
            if identifications.len() == 1 { "" } else { "s" }
        ),
        MetadataEdit::SetImportSettings { .. } => "Saved metadata import settings".into(),
    }
}

// Requests shared by the CLI, MCP and the GUI, so every route plans an
// import the same way.

/// Plan an import from a sequence file's structured headers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderImport {
    /// Project-relative path of the FASTA file.
    pub document: String,
    pub pattern: HeaderPattern,
    #[serde(default)]
    pub options: ImportOptions,
}

/// Plan an import from a spreadsheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetImport {
    /// Path of the spreadsheet (absolute, or relative to the project).
    pub path: String,
    /// Column header to target. Omitted: guessed from the headers.
    #[serde(default)]
    pub mapping: Option<BTreeMap<String, FieldTarget>>,
    /// Project-relative sequence file whose records the `record` column
    /// names. Omitted: a template's `document` column, if any.
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub options: ImportOptions,
}

/// A project-relative document key for `path`, which must be inside the
/// project and exist.
pub fn document_key(project: &Path, path: &Path) -> Result<String> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.join(path)
    };
    let full = full.canonicalize()?;
    let root = project.canonicalize()?;
    let relative = full.strip_prefix(&root).map_err(|_| {
        invalid(format!(
            "{} is outside the project; links name files inside it",
            path.display()
        ))
    })?;
    relative
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Unsupported("file names must be UTF-8".into()))
}

fn checked_metadata(session: &Session, doc: crate::DocId) -> Result<&ProjectMetadata> {
    session.metadata(doc)
}

pub fn plan_header_import(
    session: &Session,
    doc: crate::DocId,
    request: &HeaderImport,
) -> Result<Plan> {
    let meta = checked_metadata(session, doc)?;
    let key = document_key(session.root(), Path::new(&request.document))?;
    let headers = fasta_headers(&session.root().join(&key))?;
    plan_headers(meta, &key, &headers, &request.pattern, request.options)
}

pub fn plan_sheet_import(
    session: &Session,
    doc: crate::DocId,
    request: &SheetImport,
) -> Result<Plan> {
    let meta = checked_metadata(session, doc)?;
    let path = Path::new(&request.path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        session.root().join(path)
    };
    let sheet = read_sheet(&path)?;
    let mapping = request
        .mapping
        .clone()
        .unwrap_or_else(|| guess_mapping(meta, &sheet.headers));
    let document = request
        .document
        .as_deref()
        .map(|d| document_key(session.root(), Path::new(d)))
        .transpose()?;
    let mut plan = plan_sheet(meta, &sheet, &mapping, document.as_deref(), request.options)?;
    plan.mapping = mapping;
    Ok(plan)
}

/// Write the template for every record of `document` (project-relative).
pub fn template_for(
    session: &Session,
    doc: crate::DocId,
    document: &str,
    output: &Path,
) -> Result<usize> {
    let meta = checked_metadata(session, doc)?;
    let key = document_key(session.root(), Path::new(document))?;
    let records: Vec<(String, String)> = fasta_headers(&session.root().join(&key))?
        .into_iter()
        .map(|(id, _)| (key.clone(), id))
        .collect();
    write_template(meta, &records, output)?;
    Ok(records.len())
}

/// Linked and unlinked records of a sequence file. Loading never needs
/// links; this only reports them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkStatus {
    pub document: String,
    pub records: usize,
    pub unlinked: Vec<String>,
}

pub fn link_status(session: &Session, doc: crate::DocId, document: &str) -> Result<LinkStatus> {
    let meta = checked_metadata(session, doc)?;
    let key = document_key(session.root(), Path::new(document))?;
    let records: Vec<String> = fasta_headers(&session.root().join(&key))?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let unlinked = model::unlinked(meta, &key, &records)
        .into_iter()
        .map(str::to_string)
        .collect();
    Ok(LinkStatus {
        document: key,
        records: records.len(),
        unlinked,
    })
}

/// INSDC source qualifiers for a specimen, as `{qualifier: value}` plus the
/// required ones that are missing.
pub fn insdc(session: &Session, doc: crate::DocId, specimen: &str) -> Result<serde_json::Value> {
    let meta = checked_metadata(session, doc)?;
    let source = model::insdc_source(meta, specimen).map_err(|e| invalid(e.0))?;
    Ok(serde_json::json!({
        "specimen": specimen,
        "qualifiers": source.qualifiers.into_iter().collect::<BTreeMap<_, _>>(),
        "missing": source.missing,
    }))
}

/// Shared consumer routes. Plans are inspectable and applied using the normal
/// version-checked `apply` operation; this function never mutates documents.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ConsumerRequest {
    Partitions,
    /// Explicit record-ID assignments reviewed at the document workflow step.
    LinkRecords {
        document: String,
        links: BTreeMap<String, String>,
    },
    Labels {
        document: String,
        records: Vec<String>,
        fields: Vec<basecraft_protocol::metadata::LabelField>,
    },
    LinkWorkspace {
        workspace: String,
        links: BTreeMap<String, String>,
    },
    PromotePartition {
        workspace: String,
        hypothesis: String,
        id: String,
    },
}

/// The tree style that labels tree `doc`'s tips from the specimen metadata
/// of `project`, by `fields` in order (no fields: the tips' own names again),
/// and how many tips keep their own name because their link is missing or
/// ambiguous. `project` is named by the caller (the window's chosen project,
/// `--project`, the MCP session's project), since a tree opened loose has
/// none of its own. The window and `basecraft tree labels` both label tips
/// through this.
pub fn tip_label_style(
    session: &Session,
    doc: crate::DocId,
    project: &crate::project::Project,
    fields: Vec<basecraft_protocol::metadata::LabelField>,
) -> Result<(basecraft_protocol::TreeStyle, usize)> {
    let tree = session.tree(doc)?;
    let mut style = tree.style().clone();
    style.tip_labels.clear();
    style.extra.remove("metadata_labels");
    if fields.is_empty() {
        return Ok((style, 0));
    }
    // The table's records are kept in its project, so it is read there.
    let mut reader = Session::unlogged(project.root()).in_project(Some(project.clone()));
    let metadata = open(&mut reader, false)?.ok_or(Error::NoMetadata)?;
    let source = consumer_source(&reader, metadata)?;
    verify_label_owner(tree.style(), &source)?;
    let request = ConsumerRequest::Labels {
        document: session
            .root()
            .join(session.doc_key(doc)?)
            .to_string_lossy()
            .into_owned(),
        records: tree
            .nodes()
            .iter()
            .filter(|n| n.is_tip())
            .map(|n| n.name.clone())
            .collect(),
        fields,
    };
    let plan = consumer_plan(&reader, metadata, &request)?;
    let labels: Vec<basecraft_protocol::metadata::ComposedLabel> =
        serde_json::from_value(plan["labels"].clone())?;
    let kept = labels.iter().filter(|l| l.issue.is_some()).count();
    style.tip_labels = labels
        .into_iter()
        .filter(|l| l.issue.is_none())
        .map(|l| (l.record, l.label))
        .collect();
    style.extra.insert("metadata_labels".into(), plan);
    Ok((style, kept))
}

/// Label tree `doc`'s tips from the project's specimen metadata (see
/// `tip_label_style`) as one style edit; how many tips keep their own name.
pub fn label_tips(
    session: &mut Session,
    doc: crate::DocId,
    project: &crate::project::Project,
    fields: Vec<basecraft_protocol::metadata::LabelField>,
) -> Result<usize> {
    let (style, kept) = tip_label_style(session, doc, project, fields)?;
    session.apply(
        doc,
        basecraft_protocol::Op::TreeStyle {
            style,
            remove_supports: Vec::new(),
        },
    )?;
    Ok(kept)
}

/// A label field as a person writes it: `specimen`, `genus`, `species`,
/// `accession`, or `column:NAME` for a column of the specimen table.
pub fn label_field(text: &str) -> Result<basecraft_protocol::metadata::LabelField> {
    use basecraft_protocol::metadata::LabelField;
    Ok(match text.trim() {
        "specimen" => LabelField::Specimen,
        "genus" => LabelField::Genus,
        "species" => LabelField::Species,
        "accession" => LabelField::Accession,
        other => match other.strip_prefix("column:") {
            Some(name) if !name.trim().is_empty() => LabelField::Column {
                name: name.trim().to_owned(),
            },
            _ => return Err(Error::NotALabelField(other.to_owned())),
        },
    })
}

pub fn consumer_plan(
    session: &Session,
    doc: crate::DocId,
    request: &ConsumerRequest,
) -> Result<serde_json::Value> {
    use basecraft_docs::metadata_consumers;
    use basecraft_protocol::{Op, metadata::MetadataEdit};
    let meta = checked_metadata(session, doc)?;
    let metadata_hash =
        basecraft_io::hash_bytes(&serde_json::to_vec(meta)?);
    let (workspace, links, promotion) = match request {
        ConsumerRequest::Partitions => {
            return Ok(
                serde_json::json!({"partitions": metadata_consumers::partitions(meta), "metadata_hash": metadata_hash}),
            );
        }
        ConsumerRequest::LinkRecords { document, links } => {
            let key = document_key(session.root(), Path::new(document))?;
            let edit = MetadataEdit::Apply {
                changes: ChangeSet {
                    links: links
                        .iter()
                        .map(|(record, specimen)| RecordLink {
                            document: key.clone(),
                            record: record.clone(),
                            specimen: specimen.clone(),
                        })
                        .collect(),
                    ..ChangeSet::default()
                },
            };
            let mut checked = meta.clone();
            model::apply(&mut checked, &edit).map_err(|e| invalid(e.0))?;
            return Ok(
                serde_json::json!({"target":session.doc_key(doc)?, "operation": Op::MetadataEdit { edit: Box::new(edit) }, "metadata_hash": metadata_hash}),
            );
        }
        ConsumerRequest::Labels {
            document,
            records,
            fields,
        } => {
            let key = document_key(session.root(), Path::new(document))?;
            let source = consumer_source(session, doc)?;
            if matches!(
                crate::files::Format::from_path(&session.root().join(&key))?,
                crate::files::Format::Tree
            ) {
                let mut reader = Session::unlogged(session.root());
                let tree = reader.open(&session.root().join(&key))?;
                verify_label_owner(reader.tree(tree)?.style(), &source)?;
            }
            return Ok(
                serde_json::json!({"labels":metadata_consumers::compose_labels(meta, &key, records, fields), "metadata_hash":metadata_hash, "metadata_source":source, "fields":fields}),
            );
        }
        ConsumerRequest::LinkWorkspace { workspace, links } => (workspace, Some(links), None),
        ConsumerRequest::PromotePartition {
            workspace,
            hypothesis,
            id,
        } => (workspace, None, Some((hypothesis, id))),
    };
    let key = document_key(session.root(), Path::new(workspace))?;
    // A separate unlogged reader keeps planning passive and never creates records.
    let mut reader = Session::unlogged(session.root());
    let work_doc = reader.open(&session.root().join(&key))?;
    if !reader.is_delimitation(work_doc) {
        return Err(Error::MissingChoice("choose a delimitation workspace".into()));
    }
    let work = reader.delimitation(work_doc)?;
    let workspace_hash =
        basecraft_io::hash_bytes(&serde_json::to_vec(work)?);
    let (target, operation) = if let Some(links) = links {
        for roster in links.values() {
            if !meta.specimens.rows.iter().any(|s| &s.id == roster) {
                return Err(invalid(format!("unknown project roster ID {roster}")));
            }
        }
        let edit = basecraft_protocol::delimitation::DelimitationEdit::SetRosterLinks {
            links: links.clone(),
            metadata: Some(consumer_source(session, doc)?),
        };
        let mut checked = work.clone();
        basecraft_docs::delimitation::apply(&mut checked, &edit).map_err(|e| invalid(e.0))?;
        (
            key,
            Op::DelimitationEdit {
                edit: Box::new(edit),
            },
        )
    } else {
        let stored = work.exchange_fields.get(basecraft_docs::delimitation::METADATA_REFERENCE)
            .ok_or_else(|| Error::NotReady("Workspace roster links have no metadata owner; explicitly link them to this project before promoting".into()))?;
        let stored: basecraft_protocol::provenance::InputReference =
            serde_json::from_str(stored).map_err(|e| {
                damaged(
                    session.root().join(&key),
                    format!("Invalid workspace metadata reference: {e}"),
                )
            })?;
        verify_consumer_owner(&stored, &consumer_source(session, doc)?)?;
        let (hypothesis, id) = promotion.ok_or_else(|| invalid("missing partition source"))?;
        let mut partition = metadata_consumers::project_partition(meta, work, hypothesis, id)
            .map_err(|e| invalid(e.0))?;
        partition
            .provenance
            .inputs
            .insert("project_metadata".into(), metadata_hash.clone());
        partition
            .provenance
            .inputs
            .insert("source_workspace".into(), workspace_hash.clone());
        partition
            .provenance
            .parameters
            .insert("source_workspace".into(), serde_json::json!(key));
        partition.provenance.parameters.insert(
            "metadata_source".into(),
            serde_json::to_value(consumer_source(session, doc)?)?,
        );
        (
            session.doc_key(doc)?.to_string(),
            Op::MetadataEdit {
                edit: Box::new(MetadataEdit::PutPartition {
                    partition: Box::new(partition),
                }),
            },
        )
    };
    Ok(
        serde_json::json!({"target":target, "operation":operation, "metadata_hash":metadata_hash, "workspace_hash":workspace_hash}),
    )
}

/// Reuse the saved metadata source and its existing Open activity identity.
/// The activity remains in publication history across edits and moves, including
/// accepted typed TSV edits; an unrelated project's equal local IDs are insufficient.
/// No independent roster or identity allocator is introduced.
pub fn consumer_source(
    session: &Session,
    doc: crate::DocId,
) -> Result<basecraft_protocol::provenance::InputReference> {
    checked_metadata(session, doc)?;
    let (_, stamp, hash) = session
        .saved_source(doc)?
        .ok_or_else(|| {
            Error::NotReady("Save the specimen metadata before binding consumers".into())
        })?;
    let path = session.root().join(session.doc_key(doc)?);
    let (_, history) = crate::records::provenance::saved_history(&path, session.project())
        .map_err(|_| Error::NotReady("Metadata owner evidence is unavailable; save its document history before binding consumers".into()))?;
    let event = history
        .events
        .iter()
        .find(|e| matches!(e.op, basecraft_protocol::Op::Open { .. }))
        .ok_or_else(|| {
            damaged(
                &path,
                "Metadata owner Open activity is unavailable; restore its provenance",
            )
        })?;
    Ok(basecraft_protocol::provenance::InputReference {
        path: session.doc_key(doc)?.into(),
        stamp,
        hash,
        extra: BTreeMap::from([("owner_event".into(), serde_json::json!(event.id))]),
    })
}

/// Hashes identify the retained snapshot; owner activity + logical project path
/// identify the document allowed to refresh it, even after normal edits/moves.
pub fn verify_consumer_owner(
    expected: &basecraft_protocol::provenance::InputReference,
    current: &basecraft_protocol::provenance::InputReference,
) -> Result<()> {
    let owner = expected
        .extra
        .get("owner_event")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::NotReady("Stored metadata owner is missing; explicitly rebind the consumer".into())
        })?;
    if current.path != expected.path
        || current
            .extra
            .get("owner_event")
            .and_then(serde_json::Value::as_str)
            != Some(owner)
    {
        return Err(Error::NotReady(
            "Consumer belongs to different project metadata; identical specimen IDs do not establish ownership. Explicitly relink it to this project.".into(),
        ));
    }
    Ok(())
}

/// A saved display snapshot stays readable everywhere, but refreshing it must
/// not resolve equal IDs against another project's metadata.
pub fn verify_label_owner(
    style: &basecraft_protocol::TreeStyle,
    source: &basecraft_protocol::provenance::InputReference,
) -> Result<()> {
    let Some(snapshot) = style.extra.get("metadata_labels") else {
        return Ok(());
    };
    let expected = serde_json::from_value(snapshot["metadata_source"].clone())
        .map_err(|_| Error::NotReady("Label snapshot has no metadata owner; choose Original labels before explicitly composing a new snapshot".into()))?;
    verify_consumer_owner(&expected, source)
}
