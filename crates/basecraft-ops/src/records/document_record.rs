//! Revision-scoped evidence and deterministic, readable reports. Queries never write files.
use std::{collections::HashSet, fmt::Write as _, path::Path};

use basecraft_protocol::{Actor, LogEntry, Op, provenance::Publication};
use serde::{Deserialize, Serialize};

use basecraft_protocol::report_records::DocumentRecord as RecordData;
pub use basecraft_protocol::report_records::{HistoryRow, MethodsParagraph, Report};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentRecord {
    #[serde(flatten)]
    pub(crate) data: RecordData,
    #[serde(skip)]
    pub(crate) origins: Vec<(std::path::PathBuf, Publication)>,
    /// The folder run links are relative to: the project root, or the
    /// file's own folder for a loose file.
    #[serde(skip)]
    pub(crate) run_root: Option<std::path::PathBuf>,
}
impl std::ops::Deref for DocumentRecord {
    type Target = RecordData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::DerefMut for DocumentRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

pub fn read_origin(
    path: &Path,
    hashes: &[(String, String)],
    project: Option<&crate::project::Project>,
) -> (Option<Publication>, Option<String>) {
    if let Err(e) = crate::records::analysis_store::ArtifactRun::for_result(path) {
        return (
            None,
            Some(format!("Hidden analysis binding could not be read: {e}")),
        );
    }
    let bundle = crate::records::provenance::bundle_for_result(path, project);
    if !bundle.join("manifest.json").exists() {
        return (None, None);
    }
    match crate::records::provenance::inspect(path, project) {
        Ok(p) if !hashes.is_empty() && hashes.iter().all(|(_, hash)| p.outputs.iter().any(|a| &a.hash == hash)) => (Some(p), None),
        Ok(_) => (None, Some("The saved record describes different file contents. Earlier ancestry is unavailable for this revision.".into())),
        Err(e) => (None, Some(format!("The saved record could not be read: {e}"))),
    }
}

impl DocumentRecord {
    /// Complete an operation's existing inventory with the submitted input's
    /// chronological history and producing activity. Retry preserves user Methods.
    pub fn publish_operation_activity(
        &self,
        run: &crate::records::analysis_store::ArtifactRun,
        activity: &LogEntry,
    ) -> crate::Result<()> {
        let manifest = run.directory().join("manifest.json");
        let mut publication: Publication = basecraft_protocol::records::decode(
            &crate::records::provenance::read_record(&manifest)?,
        )?;
        let (events, effective) = super::publish::portable_analysis_activity(self, activity);
        publication.events = events;
        publication.effective_edits = effective;
        // Preserve upstream bundles used by the submitted revision for packaging.
        let mut input = crate::records::input_evidence::read_input(run.directory())?;
        let mut upstream = input.upstream.clone();
        for (bundle, origin) in &self.origins {
            let bytes = basecraft_protocol::records::encode(origin)?;
            let reference = basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::records::input_evidence::relative(
                    run.directory(),
                    &bundle.canonicalize()?,
                )?)
                .into(),
                id: origin.id.clone(),
                hash: basecraft_io::hash_bytes(&bytes),
            };
            if !upstream.contains(&reference) {
                upstream.push(reference);
            }
        }
        if !upstream.is_empty() {
            input.upstream = upstream;
            crate::records::input_evidence::write_json(
                &run.directory().join("input.json"),
                &input,
            )?;
        }
        crate::records::provenance::publish(run.directory(), &publication)
    }

    /// In-memory locations of contributing publication bundles. Reading them
    /// creates no records and does not expose mutable ancestry internals.
    pub fn origin_bundles(&self) -> impl Iterator<Item = &Path> {
        self.origins.iter().map(|(path, _)| path.as_path())
    }

    pub fn empty(document: &str) -> Self {
        Self {
            data: RecordData {
                schema_version: 1,
                document: document.into(),
                revision: String::new(),
                unsaved: false,
                warnings: vec!["Earlier processing before import is not recorded.".into()],
                events: Vec::new(),
                effective: Vec::new(),
            },
            origins: Vec::new(),
            run_root: None,
        }
    }

    /// `project` is the session's chosen project, or none for a loose file.
    pub fn from_file(
        path: &Path,
        project: Option<&crate::project::Project>,
    ) -> crate::Result<Self> {
        let path = path.canonicalize()?;
        // The project root for a file inside the project, as the GUI opens
        // it, so run links resolve the same way they were written.
        let root = crate::project::Project::base_of(project, &path)?;
        let mut session = crate::Session::recoverable(&root)?.in_project(project.cloned());
        let doc = session.open(&path)?;
        session.document_record(doc)
    }

    /// The report for a saved file as it is on disk. A saved Sanger review is
    /// read by its own reader and report adapter, exactly as the application
    /// shows it. Metadata and submission JSON use their own readers. Other
    /// documents open through [`Self::from_file`].
    pub fn saved_report(
        path: &Path,
        project: Option<&crate::project::Project>,
    ) -> crate::Result<Report> {
        if crate::metadata::is_manifest(path) || crate::submission::is_submission(path) {
            return Self::from_file(path, project)?.report();
        }
        if let Some(review) = crate::sanger_review::saved_review(path) {
            let review = review.map_err(|detail| crate::Error::Damaged {
                path: path.into(),
                detail,
            })?;
            return review
                .report_with_history(&Self::from_file(path, project)?)
                .map_err(crate::error::invalid);
        }
        Self::from_file(path, project)?.report()
    }

    pub fn from_publication(p: &Publication) -> Self {
        let mut record = Self::empty(
            p.outputs
                .first()
                .map_or("Document", |a| a.logical_name.as_str()),
        );
        record.revision.clone_from(&p.id);
        record.events.clone_from(&p.events);
        record.effective.clone_from(&p.effective_edits);
        crate::records::contributions::project(&record.data.events, &mut record.data.effective);
        if p.completeness.ancestry_known {
            record.warnings.clear();
        }
        if !p.completeness.execution_specification_available {
            record
                .warnings
                .push("Complete execution specifications are not available for every step.".into());
        }
        record
    }

    /// Include an explicitly contributing input revision, retaining independent origins.
    /// A run's own record: in the bundle of an origin with the run's id, or
    /// in the run folder the log names.
    fn run_record(
        &self,
        link: &basecraft_protocol::ops::RunLink,
    ) -> Option<basecraft_protocol::records::RunRecord> {
        let folder = self
            .origins
            .iter()
            .find(|(_, publication)| publication.id == link.id)
            .map(|(bundle, _)| bundle.clone())
            .or_else(|| self.run_root.as_ref().map(|root| root.join(&link.folder)))?;
        basecraft_protocol::records::decode(&std::fs::read(folder.join("execution.json")).ok()?)
            .ok()
    }

    pub fn include_input(&mut self, input: &Self) {
        self.events.extend(input.events.iter().cloned());
        self.effective.extend(input.effective.iter().cloned());
        for warning in &input.warnings {
            if !self.warnings.contains(warning) {
                self.warnings.push(warning.clone());
            }
        }
        for origin in &input.origins {
            if !self.origins.iter().any(|(_, p)| p.id == origin.1.id) {
                self.origins.push(origin.clone());
            }
        }
        self.deduplicate();
    }

    pub fn deduplicate(&mut self) {
        let mut seen = HashSet::new();
        self.events.retain(|e| seen.insert(e.id.clone()));
        seen.clear();
        self.effective.retain(|e| seen.insert(e.id.clone()));
    }

    pub fn report(&self) -> crate::Result<Report> {
        if self.events.len() > 100_000 || self.effective.len() > 100_000 {
            return Err(crate::error::invalid(
                "Document record exceeds the 100,000-event report limit",
            ));
        }
        let mut event_map = std::collections::HashMap::new();
        for event in &self.events {
            if event_map
                .insert(&event.id, &event.op)
                .is_some_and(|previous| previous != &event.op)
            {
                return Err(crate::error::invalid(
                    "Conflicting operations share a history identity",
                ));
            }
        }
        for activity in &self.effective {
            if event_map.get(&activity.id).copied() != Some(&activity.operation) {
                return Err(crate::error::invalid(
                    "A contributing step has no matching history event",
                ));
            }
        }
        let effective: HashSet<_> = self.effective.iter().map(|e| e.id.as_str()).collect();
        let mut history = Vec::new();
        let mut history_text = format!(
            "History of {}\nRevision: {}\n\n",
            self.document, self.revision
        );
        for event in &self.events {
            let contributing = effective.contains(event.id.as_str());
            let status = if contributing {
                if presentation(&event.op) {
                    "Presentation"
                } else {
                    "Contributing"
                }
            } else {
                match event.op {
                    Op::Open { .. } => "Source",
                    Op::Save { .. } => "Saved checkpoint",
                    Op::Undo { .. } | Op::Redo { .. } => "Audit",
                    Op::ToolRun { .. }
                    | Op::JobFinished { .. }
                    | Op::Export { .. }
                    | Op::Extract { .. } => "Related activity",
                    _ => "Undone / not contributing",
                }
            };
            let summary = crate::log::describe(&event.op);
            let actor = match &event.actor {
                Actor::Tool { tool, version } => format!("{tool} {version}"),
                Actor::Plugin { id, version } => format!("{id} {version}"),
                Actor::Agent { model } => format!("Agent: {model}"),
                Actor::Script { client } => format!("Script: {client}"),
                Actor::Human { .. } => "Manual action".into(),
                Actor::External => "External change".into(),
            };
            let details = format!(
                "{actor}\nDocument: {}\nActivity: {}\nTime: {}\nSummary coordinates are one-based. Exact start/end settings below and raw operations use zero-based, half-open ranges.\n\n{}",
                event.doc,
                event.id,
                event.t,
                readable_details(event)?
            );
            let _ = writeln!(history_text, "{}  {summary} [{status}]", event.t);
            history.push(HistoryRow {
                id: event.id.clone(),
                time: event.t.clone(),
                summary,
                status: status.into(),
                contributing,
                details,
            });
        }
        let mut paragraphs = Vec::new();
        let mut citations = Vec::new();
        for activity in &self.effective {
            if presentation(&activity.operation) {
                continue;
            }
            let event = self.events.iter().find(|e| e.id == activity.id);
            let text = match &activity.operation {
                Op::ToolRun {
                    plugin,
                    operation,
                    params,
                    run,
                    ..
                } => {
                    let record = run.as_ref().and_then(|link| self.run_record(link));
                    let execution = record.as_ref().and_then(|r| r.execution.as_ref());
                    for value in execution
                        .and_then(|e| e.methods.as_ref())
                        .map_or(&[][..], |m| &m.citations)
                    {
                        if !citations.contains(value) {
                            citations.push(value.clone());
                        }
                    }
                    tool_methods(plugin, operation, params, record.as_ref(), event)
                }
                Op::Open { .. }
                | Op::Save { .. }
                | Op::Undo { .. }
                | Op::Redo { .. }
                | Op::JobFinished { .. } => continue,
                op => format!(
                    "{}. This change was recorded in Basecraft.",
                    crate::log::describe(op)
                ),
            };
            paragraphs.push(MethodsParagraph {
                activity_ids: vec![activity.id.clone()],
                text,
                parameters: serde_json::to_string_pretty(&activity.operation)?,
            });
        }
        let mut methods = paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        if methods.is_empty() {
            methods =
                "No contributing scientific transformations are recorded for this revision.".into();
        }
        if !citations.is_empty() {
            methods.push_str("\n\nReferences:\n");
            methods.push_str(&citations.join("\n"));
        }
        if !self.warnings.is_empty() {
            methods.push_str("\n\nRecord limitations:\n");
            methods.push_str(&self.warnings.join("\n"));
        }
        Ok(Report {
            schema_version: 1,
            template_version: 2,
            document: self.document.clone(),
            revision: self.revision.clone(),
            unsaved: self.unsaved,
            warnings: self.warnings.clone(),
            history,
            paragraphs,
            methods,
            history_text,
            raw: serde_json::to_string_pretty(self)?,
            citations,
        })
    }

    /// Export a frozen report snapshot. Authored methods.md is never overwritten.
    pub fn export(&self, directory: &Path) -> crate::Result<()> {
        self.report()?.export(directory)
    }
}

/// Human parameter labels; exact serialized values remain in the raw record.
fn readable_details(event: &LogEntry) -> crate::Result<String> {
    fn fields(value: &serde_json::Value, prefix: &str, output: &mut String, depth: usize) {
        if depth > 8 {
            output.push_str("Additional nested settings are available in the raw record.\n");
            return;
        }
        match value {
            serde_json::Value::Object(map) => {
                for (name, value) in map {
                    let label = name.trim_start_matches('_').replace('_', " ");
                    let label = if prefix.is_empty() {
                        label
                    } else {
                        format!("{prefix} / {label}")
                    };
                    fields(value, &label, output, depth + 1);
                }
            }
            serde_json::Value::Array(items) if items.len() <= 24 => {
                for (index, item) in items.iter().enumerate() {
                    fields(item, &format!("{prefix} {}", index + 1), output, depth + 1);
                }
            }
            serde_json::Value::Array(items) => {
                let _ = writeln!(output, "{prefix}: {} entries (see raw record)", items.len());
            }
            _ => {
                let value = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_string);
                let short: String = value.chars().take(500).collect();
                let suffix = if short.len() < value.len() {
                    "… (full value in raw record)"
                } else {
                    ""
                };
                let _ = writeln!(output, "{prefix}: {short}{suffix}");
            }
        }
    }
    let value = serde_json::to_value(&event.op)?;
    let mut details = String::new();
    if let Some(params) = value.get("params") {
        fields(params, "", &mut details, 0);
    }
    for (name, hash) in &event.result.hashes {
        let _ = writeln!(details, "Recorded file: {name}\nContent hash: {hash}");
    }
    Ok(details)
}

fn presentation(op: &Op) -> bool {
    match op {
        Op::TreeStyle {
            remove_supports, ..
        } => remove_supports.is_empty(),
        Op::TreeRotate { .. }
        | Op::TreeLadderize { .. }
        | Op::TreeAnnotate { .. }
        | Op::TreePlaceLabel { .. }
        | Op::Export { .. } => true,
        _ => false,
    }
}

/// Files a report export writes. Authored `methods.md` is never among them.
const REPORT_FILES: [&str; 8] = [
    "methods.generated.md",
    "history.md",
    "record.json",
    "parameters.tsv",
    "source-map.json",
    "citations.json",
    "manifest.json",
    "report.json",
];

/// An earlier export may be refreshed; another program's files of the same
/// names may not be replaced.
fn check_report_folder(directory: &Path) -> crate::Result<()> {
    let present: Vec<_> = REPORT_FILES
        .iter()
        .filter(|name| directory.join(name).symlink_metadata().is_ok())
        .copied()
        .collect();
    if present.is_empty() {
        return Ok(());
    }
    let earlier = std::fs::read(directory.join("manifest.json"))
        .ok()
        .and_then(|bytes| basecraft_protocol::records::decode::<serde_json::Value>(&bytes).ok())
        .is_some_and(|m| {
            m["schema_version"] == 1
                && m.get("template_version").is_some()
                && m.get("coordinates").is_some()
        });
    if earlier {
        return Ok(());
    }
    Err(crate::error::invalid(format!(
        "{} already holds {} that is not from a Basecraft report export; choose an empty or new folder",
        directory.display(),
        present.join(", ")
    )))
}

pub trait ReportExt {
    fn export(&self, directory: &Path) -> crate::Result<()>;
}

impl ReportExt for Report {
    fn export(&self, directory: &Path) -> crate::Result<()> {
        check_report_folder(directory)?;
        write_report_files(self, directory).map_err(|error| {
            crate::Error::Damaged { path: directory.to_path_buf(), detail: format!(
                "report export to {} is incomplete ({error}); files there may mix this report with an earlier one",
                directory.display()
            ) }
        })
    }
}

fn write_report_files(report: &Report, directory: &Path) -> crate::Result<()> {
    std::fs::create_dir_all(directory)?;
    for (name, text) in [
        ("methods.generated.md", report.methods.as_str()),
        ("history.md", report.history_text.as_str()),
        ("record.json", report.raw.as_str()),
    ] {
        basecraft_io::write_atomic(&directory.join(name), |w| w.write_all(text.as_bytes()))?;
    }
    let source_map: Vec<_> = report
        .paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            serde_json::to_value(basecraft_protocol::report_records::ReportSourceMapEntry {
                paragraph: i + 1,
                activities: p.activity_ids.clone(),
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?;
    let mut parameters = String::from("paragraph\tactivity_ids\tparameters_json\n");
    for (i, p) in report.paragraphs.iter().enumerate() {
        let _ = writeln!(
            parameters,
            "{}\t{}\t{}",
            i + 1,
            p.activity_ids.join(","),
            serde_json::to_string(&p.parameters)?
        );
    }
    let manifest =
        serde_json::to_value(basecraft_protocol::report_records::ReportExportManifest {
            schema_version: 1,
            revision: report.revision.clone(),
            document: report.document.clone(),
            unsaved: report.unsaved,
            template_version: report.template_version,
            coordinates: "raw zero-based half-open; summaries one-based".into(),
        })?;
    for (name, bytes) in [
        ("parameters.tsv", parameters.into_bytes()),
        ("source-map.json", serde_json::to_vec_pretty(&source_map)?),
        (
            "citations.json",
            serde_json::to_vec_pretty(&report.citations)?,
        ),
        (
            "manifest.json",
            basecraft_protocol::records::encode(&manifest)?,
        ),
    ] {
        basecraft_io::write_atomic(&directory.join(name), |w| w.write_all(&bytes))?;
    }
    let bytes = basecraft_protocol::records::encode(report)?;
    basecraft_io::write_atomic(&directory.join("report.json"), |w| w.write_all(&bytes))?;
    Ok(())
}

fn tool_methods(
    plugin: &str,
    operation: &str,
    params: &serde_json::Value,
    record: Option<&basecraft_protocol::records::RunRecord>,
    event: Option<&LogEntry>,
) -> String {
    let tool = match event.map(|e| &e.actor) {
        Some(Actor::Tool { tool, version }) => format!("{tool} (version {version})"),
        Some(Actor::Plugin { id, version }) => {
            format!("plugin {id} (version {version}; external tool version not recorded)")
        }
        _ => format!("{plugin} (software version not recorded)"),
    };
    let execution = record.and_then(|record| record.execution.as_ref());
    let procedure = execution
        .and_then(|e| e.methods.as_ref())
        .map_or("A recorded analysis was performed", |m| {
            m.description.as_str()
        });
    let mut text = format!("{procedure} using {tool} (operation: {operation}).");
    for step in execution.map_or(&[][..], |e| &e.preprocessing) {
        let _ = write!(text, " {step}.");
    }
    let settings = match (execution, record) {
        (Some(execution), _) => Some(&execution.resolved_parameters),
        (None, Some(record)) => record.effective_parameters.as_object(),
        (None, None) => None,
    };
    if let Some(settings) = settings {
        if !settings.is_empty() {
            text.push_str(" Recorded effective settings: ");
            text.push_str(
                &settings
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            text.push('.');
        }
    } else {
        text.push_str(" Effective defaults were not fully recorded; submitted parameters are available in the supporting record.");
    }
    if (plugin, operation) == crate::session::support_import::ACTIVITY
        && let Ok(import) = serde_json::from_value::<
            crate::session::support_import::SupportImportParams,
        >(params.clone())
    {
        let _ = write!(
            text,
            " Support mapping: matching clades; source {}; sets {}; matched branches {}.",
            serde_json::to_string(&import.source).unwrap_or_default(),
            serde_json::to_string(&import.sets).unwrap_or_default(),
            import.matched
        );
    }
    if let Some(event) = event {
        for (name, hash) in &event.result.hashes {
            let _ = write!(text, " Input evidence: {name} ({hash}).");
        }
    }
    text
}
