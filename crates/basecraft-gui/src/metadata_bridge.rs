//! QML boundary for the project's specimen metadata (Data submission tab).
//!
//! A thin adapter over `basecraft_ops::metadata`: the GUI plans imports with
//! the same functions as the CLI and MCP, and every accepted change is one
//! typed `metadata.edit` in a recoverable session. Changes are saved as
//! soon as they are applied; undo stays available.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use basecraft_ops::metadata::{self, HeaderImport, ImportOptions, Plan, SheetImport};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::Op;
use basecraft_protocol::metadata::{
    ChangeSet, ColumnKind, FieldTarget, HeaderPattern, Identification, MetadataEdit, MissingReason,
    ProjectMetadata, SpreadsheetMapping, Taxon,
};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for MetadataController {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, project_dir)]
        #[qproperty(bool, available)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, table)]
        #[qproperty(QString, plan)]
        #[qproperty(QString, message)]
        #[qproperty(QString, message_kind)]
        #[qproperty(bool, dirty)]
        #[qproperty(bool, can_undo)]
        #[qproperty(bool, can_redo)]
        type MetadataController = super::MetadataRust;
        #[qsignal]
        #[cxx_name = "saveQuestion"]
        fn save_question(self: Pin<&mut MetadataController>);
        #[qinvokable]
        fn check_save(self: &MetadataController, path: &QString) -> QString;
        #[qinvokable]
        fn save_confirmed(
            self: Pin<&mut MetadataController>,
            path: &QString,
            token: &QString,
            policy: &QString,
            acknowledge: bool,
        ) -> bool;
        #[qinvokable]
        fn use_project(self: Pin<&mut MetadataController>, path: &QString);
        #[qsignal]
        fn opened(self: Pin<&mut MetadataController>, success: bool);
        #[qinvokable]
        fn create(self: Pin<&mut MetadataController>) -> bool;
        #[qinvokable]
        fn set_cell(
            self: Pin<&mut MetadataController>,
            specimen: &QString,
            column: &QString,
            text: &QString,
        ) -> bool;
        #[qinvokable]
        fn add_specimen(self: Pin<&mut MetadataController>, id: &QString) -> bool;
        #[qinvokable]
        fn remove_specimen(self: Pin<&mut MetadataController>, id: &QString) -> bool;
        #[qinvokable]
        fn add_column(self: Pin<&mut MetadataController>, name: &QString) -> bool;
        #[qinvokable]
        fn identify(self: Pin<&mut MetadataController>, specimen: &QString, name: &QString)
        -> bool;
        #[qinvokable]
        fn undo(self: Pin<&mut MetadataController>);
        #[qinvokable]
        fn redo(self: Pin<&mut MetadataController>);
        #[qinvokable]
        fn header_preview(
            self: &MetadataController,
            file: &QString,
            separator: &QString,
        ) -> QString;
        #[qinvokable]
        fn plan_headers(
            self: Pin<&mut MetadataController>,
            file: &QString,
            separator: &QString,
            fields: &QString,
            create_specimens: bool,
            overwrite: bool,
        ) -> bool;
        #[qinvokable]
        fn sheet_preview(self: &MetadataController, path: &QString) -> QString;
        #[qinvokable]
        fn plan_sheet(
            self: Pin<&mut MetadataController>,
            path: &QString,
            mapping: &QString,
            document: &QString,
            create_specimens: bool,
            overwrite: bool,
        ) -> bool;
        #[qinvokable]
        fn apply_plan(self: Pin<&mut MetadataController>, remember: &QString) -> bool;
        #[qinvokable]
        fn inclusion_presets(self: Pin<&mut MetadataController>, save: &QString) -> QString;
        #[qinvokable]
        fn clear_plan(self: Pin<&mut MetadataController>);
        #[qinvokable]
        fn write_template(
            self: Pin<&mut MetadataController>,
            file: &QString,
            output: &QString,
        ) -> bool;
        #[qinvokable]
        fn link_status(self: &MetadataController, file: &QString) -> QString;
        #[qinvokable]
        fn consumer(self: Pin<&mut MetadataController>, request: &QString, apply: bool) -> QString;
    }
}

/// What the last plan was for, so applying it can remember its settings.
enum PlanSource {
    Headers(HeaderPattern),
    Sheet(BTreeMap<String, FieldTarget>),
}

// Each bool is a QML property.
#[allow(clippy::struct_excessive_bools)]
pub struct MetadataRust {
    project_dir: QString,
    available: bool,
    busy: bool,
    open_epoch: u64,
    table: QString,
    plan: QString,
    message: QString,
    message_kind: QString,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
    session: Option<Session>,
    doc: Option<DocId>,
    pending: Option<(Plan, PlanSource)>,
}

impl Default for MetadataRust {
    fn default() -> Self {
        Self {
            project_dir: QString::default(),
            available: false,
            busy: false,
            open_epoch: 0,
            table: QString::from(table_json(None, &[]).to_string().as_str()),
            plan: QString::default(),
            message: QString::default(),
            message_kind: QString::default(),
            dirty: false,
            can_undo: false,
            can_redo: false,
            session: None,
            doc: None,
            pending: None,
        }
    }
}

fn kind_name(kind: &ColumnKind) -> &'static str {
    match kind {
        ColumnKind::Text => "text",
        ColumnKind::Date => "date",
        ColumnKind::Latitude => "latitude",
        ColumnKind::Longitude => "longitude",
        ColumnKind::Number { .. } => "number",
        ColumnKind::Categorical { .. } => "categorical",
    }
}

/// The table as QML shows it: columns, rows with text values, the current
/// taxon and link count, columns that can still be added, and saved import
/// settings.
pub(crate) fn table_json(meta: Option<&ProjectMetadata>, notices: &[String]) -> Value {
    let Some(meta) = meta else {
        return json!({"available": false, "columns": [], "rows": [], "addable": [], "notices": notices,
            "patterns": [], "mappings": [], "missing": MissingReason::ALL.iter().map(|r| r.as_str()).collect::<Vec<_>>()});
    };
    let columns: Vec<Value> = meta
        .specimens
        .columns
        .iter()
        .map(|c| {
            json!({"name": c.name, "type": kind_name(&c.kind), "description": c.description,
                "unit": match &c.kind { ColumnKind::Number { unit } => unit.clone(), _ => None },
                "levels": match &c.kind { ColumnKind::Categorical { levels } => levels.clone(), _ => Vec::new() }})
        })
        .collect();
    let rows: Vec<Value> = meta
        .specimens
        .rows
        .iter()
        .map(|row| {
            let taxon = basecraft_docs::metadata::current_identification(meta, &row.id)
                .and_then(|i| basecraft_docs::metadata::taxon(meta, &i.taxon))
                .map(|t| t.name.clone())
                .unwrap_or_default();
            let cells: BTreeMap<&str, String> = row
                .cells
                .iter()
                .map(|(name, cell)| (name.as_str(), basecraft_docs::metadata::format_cell(cell)))
                .collect();
            json!({"id": row.id, "cells": cells, "taxon": taxon,
                "identifications": meta.identifications.iter().filter(|i| i.specimen == row.id).count(),
                "links": meta.links.iter().filter(|l| l.specimen == row.id).count()})
        })
        .collect();
    let addable: Vec<Value> = metadata::KNOWN_COLUMNS
        .iter()
        .filter(|k| !meta.specimens.columns.iter().any(|c| c.name == k.name))
        .map(|k| json!({"name": k.name, "description": k.description}))
        .collect();
    json!({"available": true, "columns": columns, "rows": rows, "addable": addable,
        "partitions": basecraft_docs::metadata_consumers::partitions(meta),
        "notices": notices, "taxa": meta.taxa.len(), "links": meta.links.len(),
        "patterns": meta.import.header_patterns, "mappings": meta.import.spreadsheet_mappings,
        "missing": MissingReason::ALL.iter().map(|r| r.as_str()).collect::<Vec<_>>()})
}

/// The first records of a FASTA file split by `separator`, for the field
/// role editor.
pub(crate) fn header_preview(headers: &[(String, String)], separator: &str) -> Value {
    let rows: Vec<Value> = headers
        .iter()
        .take(12)
        .map(|(record, header)| {
            let fields: Vec<&str> = if separator.is_empty() {
                vec![header.as_str()]
            } else {
                header.split(separator).map(str::trim).collect()
            };
            json!({"record": record, "header": header, "fields": fields})
        })
        .collect();
    let width = rows
        .iter()
        .map(|r| r["fields"].as_array().map_or(0, Vec::len))
        .max()
        .unwrap_or(0);
    json!({"records": headers.len(), "rows": rows, "fields": width})
}

/// A field role as QML names it: `specimen`, `taxon`, `record`, `ignore`
/// or `column:<name>`.
pub(crate) fn field_target(name: &str) -> Result<FieldTarget, String> {
    Ok(match name {
        "specimen" => FieldTarget::Specimen,
        "taxon" => FieldTarget::Taxon,
        "record" => FieldTarget::Record,
        "ignore" | "" => FieldTarget::Ignore,
        other => match other.strip_prefix("column:") {
            Some(column) if metadata::valid_column_name(column) => FieldTarget::Column {
                column: column.into(),
            },
            _ => return Err(format!("{other:?} is not a field role")),
        },
    })
}

pub(crate) fn field_name(target: &FieldTarget) -> String {
    match target {
        FieldTarget::Specimen => "specimen".into(),
        FieldTarget::Taxon => "taxon".into(),
        FieldTarget::Record => "record".into(),
        FieldTarget::Ignore => "ignore".into(),
        FieldTarget::Column { column } => format!("column:{column}"),
    }
}

/// A plan as the review screen shows it.
pub(crate) fn plan_json(plan: &Plan) -> Value {
    json!({"summary": metadata::summary(&plan.changes), "empty": plan.changes.is_empty(),
        "rows": plan.rows, "matched": plan.matched,
        "new_specimens": plan.changes.specimens, "values": plan.changes.cells.len(),
        "links": plan.changes.links.len(), "identifications": plan.changes.identifications.len(),
        "columns": plan.changes.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "issues": plan.issues})
}

fn qs(value: &Value) -> QString {
    QString::from(value.to_string().as_str())
}

impl MetadataRust {
    fn project(&self) -> PathBuf {
        PathBuf::from(self.project_dir.to_string())
    }

    fn current(&self) -> Result<(&Session, DocId), String> {
        match (&self.session, self.doc) {
            (Some(session), Some(doc)) => Ok((session, doc)),
            _ => Err("Choose a project folder and create its specimen table first".into()),
        }
    }
}

impl qobject::MetadataController {
    #[cfg(test)]
    #[allow(dead_code)] // Read by the real-shell bootstrap test.
    pub(crate) fn open_identity(&self) -> Option<(String, DocId, u64, u64)> {
        let session = self.rust().session.as_ref()?;
        let doc = self.rust().doc?;
        if !session.is_open(doc) {
            return None;
        }
        Some((
            session.id().into(),
            doc,
            session.generation(doc).unwrap(),
            session.revision(doc).unwrap(),
        ))
    }

    fn report(mut self: Pin<&mut Self>, kind: &str, text: &str) {
        self.as_mut().set_message_kind(QString::from(kind));
        self.set_message(QString::from(text));
    }

    fn refresh(mut self: Pin<&mut Self>) {
        if let Some(warning) =
            crate::document_open::recovery_notice(self.rust().session.as_ref(), self.rust().doc)
        {
            self.as_mut().set_message_kind(QString::from("error"));
            self.as_mut().set_message(warning);
        }
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.metadata(doc));
        }
        let (table, available, undo, redo) = match (&self.rust().session, self.rust().doc) {
            (Some(session), Some(doc)) => (
                table_json(
                    Some(crate::bridge::checked_document!(sync self,
                        session.metadata(doc)
                    )),
                    crate::bridge::checked_document!(sync self, session.metadata_notices(doc)),
                ),
                true,
                session.can_undo(doc).unwrap_or(false),
                session.can_redo(doc).unwrap_or(false),
            ),
            _ => (table_json(None, &[]), false, false, false),
        };
        self.as_mut().set_table(qs(&table));
        self.as_mut().set_available(available);
        self.as_mut().set_can_undo(undo);
        self.set_can_redo(redo);
    }

    /// Open the project's metadata if it has any. Creates nothing.
    pub fn consumer(mut self: Pin<&mut Self>, request: &QString, apply: bool) -> QString {
        let planned = (|| -> Result<Value, String> {
            let (session, doc) = self.rust().current()?;
            let request = serde_json::from_str(&request.to_string()).map_err(|e| e.to_string())?;
            metadata::consumer_plan(session, doc, &request).map_err(|e| e.to_string())
        })();
        let value = match planned {
            Ok(value) => value,
            Err(error) => return qs(&json!({"error":error})),
        };
        if apply {
            let edit = serde_json::from_value::<Op>(value["operation"].clone());
            match edit {
                Ok(Op::MetadataEdit { edit }) => {
                    if !self.as_mut().edit(vec![*edit], "Saved specimen metadata")
                        || self.rust().dirty
                    {
                        return qs(&json!({"error":self.rust().message.to_string()}));
                    }
                }
                _ => return qs(&json!({"error":"Apply workspace links in their workspace"})),
            }
        }
        qs(&value)
    }

    pub fn use_project(mut self: Pin<&mut Self>, path: &QString) {
        self.as_mut().rust_mut().open_epoch += 1;
        self.as_mut().set_busy(false);
        self.as_mut().set_project_dir(path.clone());
        {
            let mut rust = self.as_mut().rust_mut();
            rust.session = None;
            rust.doc = None;
            rust.pending = None;
        }
        self.as_mut().set_plan(QString::default());
        self.as_mut().set_dirty(false);
        let project = self.rust().project();
        if path.is_empty() || !metadata::manifest_path(&project).is_file() {
            self.as_mut().report("", "");
            self.refresh();
            return;
        }
        let epoch = self.rust().open_epoch;
        self.as_mut().set_busy(true);
        self.as_mut().refresh();
        let path = metadata::manifest_path(&project)
            .to_string_lossy()
            .into_owned();
        if let Err(error) = crate::document_open::start(
            self.qt_thread(),
            path,
            Some(project.clone()),
            move |mut this, result| {
                if this.rust().open_epoch != epoch || this.rust().project() != project {
                    return;
                }
                let ok = result.is_ok();
                match result {
                    Ok(candidate) => {
                        let mut state = this.as_mut().rust_mut();
                        state.session = Some(candidate.session);
                        state.doc = Some(candidate.doc);
                        this.as_mut().report("", "");
                    }
                    Err(error) => this.as_mut().report("error", &error.to_string()),
                }
                this.as_mut().refresh();
                this.as_mut().set_busy(false);
                this.opened(ok);
            },
        ) {
            self.as_mut().set_busy(false);
            self.as_mut().report("error", &error.to_string());
            self.opened(false);
        }
    }

    /// Create empty tables in the project's `metadata/` folder and open them.
    pub fn create(mut self: Pin<&mut Self>) -> bool {
        let project = self.rust().project();
        if self.rust().project_dir.is_empty() {
            self.report("error", "Choose a project folder first");
            return false;
        }
        // Creation has a fixed-size empty payload; callers can immediately
        // import into it. Existing project documents use the worker above.
        let result = metadata::create(&project).and_then(|path| {
            let mut session = Session::recoverable(&project)?;
            let doc = session.open(&path)?;
            Ok((session, doc))
        });
        match result {
            Ok((session, doc)) => {
                let mut state = self.as_mut().rust_mut();
                state.open_epoch += 1;
                state.session = Some(session);
                state.doc = Some(doc);
                state.pending = None;
                self.as_mut().set_busy(false);
                self.as_mut()
                    .report("ok", "Created the project's specimen table");
                self.refresh();
                true
            }
            Err(error) => {
                self.report("error", &error.to_string());
                false
            }
        }
    }

    /// Apply one edit and save, reporting the outcome.
    fn edit(mut self: Pin<&mut Self>, edits: Vec<MetadataEdit>, done: &str) -> bool {
        let result = {
            let mut rust = self.as_mut().rust_mut();
            let Some(doc) = rust.doc else {
                self.report("error", "There is no specimen table yet");
                return false;
            };
            let Some(session) = rust.session.as_mut() else {
                return false;
            };
            session.begin_group();
            let applied = edits.into_iter().try_for_each(|edit| {
                session
                    .apply(
                        doc,
                        Op::MetadataEdit {
                            edit: Box::new(edit),
                        },
                    )
                    .map(|_| ())
            });
            session.end_group();
            applied.map(|()| session.save(doc, None))
        };
        let ok = match result {
            Err(error) => {
                self.as_mut().report("error", &error.to_string());
                false
            }
            Ok(Err(error)) => {
                self.as_mut().set_dirty(true);
                self.as_mut().save_error(&error);
                true
            }
            Ok(Ok(())) => {
                self.as_mut().set_dirty(false);
                self.as_mut().report("ok", done);
                true
            }
        };
        self.refresh();
        ok
    }

    pub fn set_cell(
        mut self: Pin<&mut Self>,
        specimen: &QString,
        column: &QString,
        text: &QString,
    ) -> bool {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.metadata(doc));
        }
        let (specimen, column, text) = (specimen.to_string(), column.to_string(), text.to_string());
        let change = match self.rust().current() {
            Ok((session, doc)) => {
                let meta = crate::bridge::checked_document!(sync self, session.metadata(doc));
                if metadata::cell_text(meta, &specimen, &column) == text.trim() {
                    return true;
                }
                metadata::cell_change(meta, &specimen, &column, &text).map_err(|e| e.to_string())
            }
            Err(error) => Err(error),
        };
        match change {
            Ok(change) => self.edit(
                vec![MetadataEdit::Apply {
                    changes: ChangeSet {
                        cells: vec![change],
                        ..ChangeSet::default()
                    },
                }],
                &format!("Set {column} of {specimen}"),
            ),
            Err(error) => {
                self.report("error", &format!("{column} of {specimen}: {error}"));
                false
            }
        }
    }

    pub fn add_specimen(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string().trim().to_string();
        self.edit(
            vec![MetadataEdit::Apply {
                changes: ChangeSet {
                    specimens: vec![id.clone()],
                    ..ChangeSet::default()
                },
            }],
            &format!("Added specimen {id}"),
        )
    }

    pub fn remove_specimen(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string();
        self.edit(
            vec![MetadataEdit::RemoveSpecimens {
                specimens: [id.clone()].into(),
            }],
            &format!("Removed specimen {id}"),
        )
    }

    /// Add a known Darwin Core column, or a text column of that name.
    pub fn add_column(self: Pin<&mut Self>, name: &QString) -> bool {
        let name = name.to_string().trim().to_string();
        if !metadata::valid_column_name(&name) {
            self.report(
                "error",
                "Column names start with a letter and use letters, digits or underscores",
            );
            return false;
        }
        let column = metadata::KNOWN_COLUMNS
            .iter()
            .find(|k| k.name == name)
            .map_or_else(
                || basecraft_protocol::metadata::Column {
                    name: name.clone(),
                    kind: ColumnKind::Text,
                    description: String::new(),
                },
                metadata::KnownColumn::column,
            );
        self.edit(
            vec![MetadataEdit::Apply {
                changes: ChangeSet {
                    columns: vec![column],
                    ..ChangeSet::default()
                },
            }],
            &format!("Added column {name}"),
        )
    }

    /// Record a new identification. An existing taxon of that name is reused.
    pub fn identify(mut self: Pin<&mut Self>, specimen: &QString, name: &QString) -> bool {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.metadata(doc));
        }
        let (specimen, name) = (specimen.to_string(), name.to_string().trim().to_string());
        let changes = match self.rust().current() {
            Ok((session, doc)) => {
                let meta = crate::bridge::checked_document!(sync self, session.metadata(doc));
                let existing = basecraft_docs::metadata::taxon_by_name(meta, &name);
                let current = basecraft_docs::metadata::current_identification(meta, &specimen);
                if existing.is_some_and(|t| current.is_some_and(|i| i.taxon == t.id)) {
                    return true;
                }
                let mut changes = ChangeSet::default();
                let taxon = existing.map_or_else(
                    || {
                        let id = ulid::Ulid::generate().to_string();
                        changes.taxa.push(Taxon {
                            id: id.clone(),
                            name: name.clone(),
                            submitted_name: None,
                            taxid: None,
                        });
                        id
                    },
                    |t| t.id.clone(),
                );
                changes.identifications.push(Identification {
                    id: ulid::Ulid::generate().to_string(),
                    specimen: specimen.clone(),
                    taxon,
                    identified_by: None,
                    date: None,
                    remarks: None,
                });
                changes
            }
            Err(error) => {
                self.report("error", &error);
                return false;
            }
        };
        self.edit(
            vec![MetadataEdit::Apply { changes }],
            &format!("Identified {specimen} as {name}"),
        )
    }

    fn history(mut self: Pin<&mut Self>, undo: bool) {
        let result = {
            let mut rust = self.as_mut().rust_mut();
            let Some(doc) = rust.doc else { return };
            let Some(session) = rust.session.as_mut() else {
                return;
            };
            let step = if undo {
                session.undo(doc)
            } else {
                session.redo(doc)
            };
            step.map(|_| session.save(doc, None))
        };
        match result {
            Err(error) => self.as_mut().report("error", &error.to_string()),
            Ok(Err(error)) => {
                self.as_mut().set_dirty(true);
                self.as_mut().save_error(&error);
            }
            Ok(Ok(())) => {
                self.as_mut().set_dirty(false);
                self.as_mut()
                    .report("ok", if undo { "Undone" } else { "Redone" });
            }
        }
        self.refresh();
    }

    pub fn undo(self: Pin<&mut Self>) {
        self.history(true);
    }

    pub fn redo(self: Pin<&mut Self>) {
        self.history(false);
    }

    pub fn header_preview(&self, file: &QString, separator: &QString) -> QString {
        match metadata::fasta_headers(Path::new(&file.to_string())) {
            Ok(headers) => qs(&header_preview(&headers, &separator.to_string())),
            Err(error) => qs(&json!({"error": error.to_string()})),
        }
    }

    fn set_pending(mut self: Pin<&mut Self>, plan: Result<(Plan, PlanSource), String>) -> bool {
        match plan {
            Ok((plan, source)) => {
                self.as_mut().set_plan(qs(&plan_json(&plan)));
                self.as_mut().rust_mut().pending = Some((plan, source));
                self.report("", "");
                true
            }
            Err(error) => {
                self.as_mut().set_plan(QString::default());
                self.as_mut().rust_mut().pending = None;
                self.report("error", &error);
                false
            }
        }
    }

    pub fn plan_headers(
        self: Pin<&mut Self>,
        file: &QString,
        separator: &QString,
        fields: &QString,
        create_specimens: bool,
        overwrite: bool,
    ) -> bool {
        let plan = (|| {
            let (session, doc) = self.rust().current()?;
            let names: Vec<String> =
                serde_json::from_str(&fields.to_string()).map_err(|e| e.to_string())?;
            let pattern = HeaderPattern {
                name: "Headers".into(),
                separator: separator.to_string(),
                fields: names
                    .iter()
                    .map(|n| field_target(n))
                    .collect::<Result<_, _>>()?,
            };
            let request = HeaderImport {
                document: file.to_string(),
                pattern: pattern.clone(),
                options: ImportOptions {
                    create_specimens,
                    overwrite,
                },
            };
            metadata::plan_header_import(session, doc, &request)
                .map(|plan| (plan, PlanSource::Headers(pattern)))
                .map_err(|e| e.to_string())
        })();
        self.set_pending(plan)
    }

    /// Headers, the first rows and the mapping to start from: a remembered
    /// mapping covering these headers, else a guess.
    pub fn sheet_preview(&self, path: &QString) -> QString {
        let sheet = match metadata::read_sheet(Path::new(&path.to_string())) {
            Ok(sheet) => sheet,
            Err(error) => return qs(&json!({"error": error.to_string()})),
        };
        let empty = ProjectMetadata::default();
        let meta = match self.current() {
            Ok((session, doc)) => match session.metadata(doc) {
                Ok(metadata) => metadata,
                Err(error) => return qs(&json!({"error": error.to_string()})),
            },
            Err(_) => &empty,
        };
        let remembered = meta.import.spreadsheet_mappings.iter().rev().find(|m| {
            sheet
                .headers
                .iter()
                .filter(|h| !h.is_empty())
                .all(|h| m.columns.contains_key(h))
        });
        let mapping = remembered.map_or_else(
            || metadata::guess_mapping(meta, &sheet.headers),
            |m| m.columns.clone(),
        );
        let mapping: BTreeMap<&String, String> =
            mapping.iter().map(|(h, t)| (h, field_name(t))).collect();
        qs(
            &json!({"headers": sheet.headers, "rows": sheet.rows.iter().take(8).collect::<Vec<_>>(),
            "count": sheet.rows.len(), "mapping": mapping,
            "remembered": remembered.map(|m| m.name.clone()),
            "has_document": sheet.column("document").is_some()}),
        )
    }

    pub fn plan_sheet(
        self: Pin<&mut Self>,
        path: &QString,
        mapping: &QString,
        document: &QString,
        create_specimens: bool,
        overwrite: bool,
    ) -> bool {
        let plan = (|| {
            let (session, doc) = self.rust().current()?;
            let names: BTreeMap<String, String> =
                serde_json::from_str(&mapping.to_string()).map_err(|e| e.to_string())?;
            let mapping = names
                .iter()
                .map(|(h, n)| field_target(n).map(|t| (h.clone(), t)))
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let request = SheetImport {
                path: path.to_string(),
                mapping: Some(mapping.clone()),
                document: (!document.is_empty()).then(|| document.to_string()),
                options: ImportOptions {
                    create_specimens,
                    overwrite,
                },
            };
            metadata::plan_sheet_import(session, doc, &request)
                .map(|plan| (plan, PlanSource::Sheet(mapping)))
                .map_err(|e| e.to_string())
        })();
        self.set_pending(plan)
    }

    /// Apply the reviewed plan as one undo step. A non-empty `remember`
    /// saves its header pattern or column mapping under that name.
    /// Inclusion presets belong to this already-open metadata owner, keeping its
    /// undo stack, dirty state and accepted disk revision coherent with specimen edits.
    pub fn inclusion_presets(mut self: Pin<&mut Self>, save: &QString) -> QString {
        if !save.is_empty() {
            let planned = (|| -> Result<_, String> {
                let rule: basecraft_protocol::submission::InclusionRule =
                    serde_json::from_str(&save.to_string()).map_err(|e| e.to_string())?;
                basecraft_ops::submission::inclusion_preset_edit(
                    &basecraft_protocol::metadata::ProjectMetadata::default(),
                    rule.clone(),
                )
                .map_err(|e| e.to_string())?;
                Ok(rule)
            })();
            let rule = match planned {
                Ok(rule) => rule,
                Err(error) => return qs(&json!({"error": error})),
            };
            if self.rust().doc.is_none() && !self.as_mut().create() {
                return qs(&json!({"error": self.rust().message.to_string()}));
            }
            let edit = self.rust().current().and_then(|(session, doc)| {
                basecraft_ops::submission::inclusion_preset_edit(
                    session.metadata(doc).map_err(|e| e.to_string())?,
                    rule,
                )
                .map_err(|e| e.to_string())
            });
            match edit {
                Ok(edit) => {
                    if !self.as_mut().edit(vec![edit], "Saved inclusion preset")
                        || self.rust().dirty
                    {
                        return qs(&json!({"error": self.rust().message.to_string()}));
                    }
                }
                Err(error) => return qs(&json!({"error": error})),
            }
        }
        match self.rust().current() {
            Ok((session, doc)) => match session.metadata(doc) {
                Ok(metadata) => qs(&json!(metadata.import.inclusion_rules)),
                Err(error) => qs(&json!({"error": error.to_string()})),
            },
            Err(_) => qs(&json!([])),
        }
    }

    pub fn apply_plan(mut self: Pin<&mut Self>, remember: &QString) -> bool {
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.metadata(doc));
        }
        let Some((plan, source)) = self.as_mut().rust_mut().pending.take() else {
            self.report("error", "There is no import to apply");
            return false;
        };
        let summary = metadata::summary(&plan.changes);
        let mut edits = Vec::new();
        if !plan.changes.is_empty() {
            edits.push(MetadataEdit::Apply {
                changes: plan.changes,
            });
        }
        let name = remember.to_string().trim().to_string();
        if !name.is_empty()
            && let Ok((session, doc)) = self.rust().current()
        {
            let mut settings = crate::bridge::checked_document!(sync self, session.metadata(doc))
                .import
                .clone();
            match source {
                PlanSource::Headers(mut pattern) => {
                    pattern.name.clone_from(&name);
                    settings.header_patterns.retain(|p| p.name != name);
                    settings.header_patterns.push(pattern);
                }
                PlanSource::Sheet(columns) => {
                    settings.spreadsheet_mappings.retain(|m| m.name != name);
                    settings.spreadsheet_mappings.push(SpreadsheetMapping {
                        name: name.clone(),
                        columns,
                    });
                }
            }
            edits.push(MetadataEdit::SetImportSettings { settings });
        }
        if edits.is_empty() {
            self.as_mut().set_plan(QString::default());
            self.report("", "The import changes nothing");
            return true;
        }
        let ok = self.as_mut().edit(edits, &format!("Imported {summary}"));
        self.set_plan(QString::default());
        ok
    }

    pub fn clear_plan(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().pending = None;
        self.set_plan(QString::default());
    }

    pub fn write_template(self: Pin<&mut Self>, file: &QString, output: &QString) -> bool {
        let result = self.rust().current().and_then(|(session, doc)| {
            metadata::template_for(
                session,
                doc,
                &file.to_string(),
                Path::new(&output.to_string()),
            )
            .map_err(|e| e.to_string())
        });
        match result {
            Ok(records) => {
                self.report(
                    "ok",
                    &format!("Wrote a template with {records} records to {output}"),
                );
                true
            }
            Err(error) => {
                self.report("error", &error);
                false
            }
        }
    }

    /// `{records, unlinked: [...]}` for a sequence file, or `{}` when it
    /// can't be checked (not FASTA, outside the project, no table).
    pub fn link_status(&self, file: &QString) -> QString {
        let Ok((session, doc)) = self.rust().current() else {
            return QString::from("{}");
        };
        metadata::link_status(session, doc, &file.to_string())
            .ok()
            .and_then(|status| serde_json::to_value(status).ok())
            .map_or_else(|| QString::from("{}"), |value| qs(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_roles_round_trip_their_qml_names() {
        for name in ["specimen", "taxon", "record", "ignore", "column:eventDate"] {
            assert_eq!(field_name(&field_target(name).unwrap()), name);
        }
        assert!(field_target("column:1bad").is_err());
        assert!(field_target("nonsense").is_err());
    }

    #[test]
    fn previews_split_headers_and_report_width() {
        let headers = vec![
            ("a|b".to_string(), "a|b|2019".to_string()),
            ("c".to_string(), "c".to_string()),
        ];
        let preview = header_preview(&headers, "|");
        assert_eq!(preview["fields"], 3);
        assert_eq!(preview["rows"][0]["fields"], json!(["a", "b", "2019"]));
        assert_eq!(header_preview(&headers, "")["fields"], 1);
    }

    #[test]
    fn table_lists_rows_taxa_and_addable_columns() {
        let mut meta = ProjectMetadata::default();
        basecraft_docs::metadata::apply(
            &mut meta,
            &MetadataEdit::Apply {
                changes: ChangeSet {
                    columns: vec![metadata::KNOWN_COLUMNS[0].column()],
                    specimens: vec!["HV1".into()],
                    ..ChangeSet::default()
                },
            },
        )
        .unwrap();
        let table = table_json(Some(&meta), &["edited".into()]);
        assert_eq!(table["rows"][0]["id"], "HV1");
        assert_eq!(table["columns"][0]["name"], "catalogNumber");
        assert!(
            !table["addable"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["name"] == "catalogNumber")
        );
        assert_eq!(table["notices"][0], "edited");
        assert_eq!(table_json(None, &[])["available"], false);
    }
}

crate::editor_save::auto_save!(qobject::MetadataController, set_dirty);
