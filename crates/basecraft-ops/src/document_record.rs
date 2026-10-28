//! Revision-scoped evidence and deterministic, readable reports. Queries never write files.
use std::{collections::HashSet, fmt::Write as _, path::Path};

use basecraft_protocol::{
    Actor, LogEntry, Op,
    provenance::{EditActivity, Publication},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub schema_version: u32,
    pub document: String,
    pub revision: String,
    pub unsaved: bool,
    pub warnings: Vec<String>,
    pub events: Vec<LogEntry>,
    pub effective: Vec<EditActivity>,
    /// In-memory locations of immutable parents; paths are not part of portable reports.
    #[serde(skip)]
    pub(crate) origins: Vec<(std::path::PathBuf, Publication)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: String,
    pub time: String,
    pub summary: String,
    pub status: String,
    pub contributing: bool,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MethodsParagraph {
    pub activity_ids: Vec<String>,
    pub text: String,
    pub parameters: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub template_version: u32,
    pub document: String,
    pub revision: String,
    pub unsaved: bool,
    pub warnings: Vec<String>,
    pub history: Vec<HistoryRow>,
    pub paragraphs: Vec<MethodsParagraph>,
    pub methods: String,
    pub history_text: String,
    pub raw: String,
    #[serde(default)]
    pub citations: Vec<String>,
}

pub fn read_origin(
    path: &Path,
    hashes: &[(String, String)],
) -> (Option<Publication>, Option<String>) {
    if let Err(e) = crate::analysis_store::ArtifactRun::for_result(path) {
        return (
            None,
            Some(format!("Hidden analysis binding could not be read: {e}")),
        );
    }
    let bundle = crate::provenance::bundle_for_result(path);
    if !bundle.join("manifest.json").exists() {
        return (None, None);
    }
    match crate::provenance::inspect(path) {
        Ok(p) if !hashes.is_empty() && hashes.iter().all(|(_, hash)| p.outputs.iter().any(|a| &a.hash == hash)) => (Some(p), None),
        Ok(_) => (None, Some("The saved record describes different file contents. Earlier ancestry is unavailable for this revision.".into())),
        Err(e) => (None, Some(format!("The saved record could not be read: {e}"))),
    }
}

impl DocumentRecord {
    /// In-memory locations of contributing publication bundles. Reading them
    /// creates no records and does not expose mutable ancestry internals.
    pub fn origin_bundles(&self) -> impl Iterator<Item = &Path> {
        self.origins.iter().map(|(path, _)| path.as_path())
    }

    pub fn empty(document: &str) -> Self {
        Self {
            schema_version: 1,
            document: document.into(),
            revision: String::new(),
            unsaved: false,
            warnings: vec!["Earlier processing before import is not recorded.".into()],
            events: Vec::new(),
            effective: Vec::new(),
            origins: Vec::new(),
        }
    }

    pub fn from_file(path: &Path) -> crate::Result<Self> {
        let path = path.canonicalize()?;
        let root = path
            .parent()
            .ok_or_else(|| crate::error::invalid("input has no parent"))?;
        let mut session = crate::Session::recoverable(root)?;
        let doc = session.open(&path)?;
        session.document_record(doc)
    }

    /// The report for a saved file as it is on disk. A saved Sanger review is
    /// read by its own reader and report adapter, exactly as the application
    /// shows it; any other `.json` file is refused with the reason. Other
    /// documents open through [`Self::from_file`].
    pub fn saved_report(path: &Path) -> crate::Result<Report> {
        if let Some(review) = crate::sanger_review::saved_review(path) {
            let review = review.map_err(crate::error::invalid)?;
            // A checkpoint recovered from application state is unsaved work.
            return review
                .document_report(review.recovery_path().is_some())
                .map_err(crate::error::invalid);
        }
        Self::from_file(path)?.report()
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
                    ..
                } => {
                    if let Some(values) = params
                        .pointer("/_execution/methods/citations")
                        .and_then(serde_json::Value::as_array)
                    {
                        for value in values.iter().filter_map(serde_json::Value::as_str) {
                            if !citations.iter().any(|c| c == value) {
                                citations.push(value.to_string());
                            }
                        }
                    }
                    tool_methods(plugin, operation, params, event)
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
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
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

impl Report {
    pub fn export(&self, directory: &Path) -> crate::Result<()> {
        check_report_folder(directory)?;
        self.write_files(directory).map_err(|error| {
            crate::error::invalid(format!(
                "report export to {} is incomplete ({error}); files there may mix this report with an earlier one",
                directory.display()
            ))
        })
    }

    fn write_files(&self, directory: &Path) -> crate::Result<()> {
        std::fs::create_dir_all(directory)?;
        for (name, text) in [
            ("methods.generated.md", self.methods.as_str()),
            ("history.md", self.history_text.as_str()),
            ("record.json", self.raw.as_str()),
        ] {
            basecraft_io::write_atomic(&directory.join(name), |w| w.write_all(text.as_bytes()))?;
        }
        let source_map: Vec<_> = self
            .paragraphs
            .iter()
            .enumerate()
            .map(|(i, p)| serde_json::json!({"paragraph":i+1,"activities":p.activity_ids}))
            .collect();
        let mut parameters = String::from("paragraph\tactivity_ids\tparameters_json\n");
        for (i, p) in self.paragraphs.iter().enumerate() {
            let _ = writeln!(
                parameters,
                "{}\t{}\t{}",
                i + 1,
                p.activity_ids.join(","),
                serde_json::to_string(&p.parameters)?
            );
        }
        let manifest = serde_json::json!({"schema_version":1,"revision":self.revision,"document":self.document,"unsaved":self.unsaved,"template_version":self.template_version,"coordinates":"raw zero-based half-open; summaries one-based"});
        for (name, bytes) in [
            ("parameters.tsv", parameters.into_bytes()),
            ("source-map.json", serde_json::to_vec_pretty(&source_map)?),
            (
                "citations.json",
                serde_json::to_vec_pretty(&self.citations)?,
            ),
            ("manifest.json", serde_json::to_vec_pretty(&manifest)?),
        ] {
            basecraft_io::write_atomic(&directory.join(name), |w| w.write_all(&bytes))?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        basecraft_io::write_atomic(&directory.join("report.json"), |w| w.write_all(&bytes))?;
        Ok(())
    }
}

/// Retained job evidence is independent of open tabs. Reading creates no directories.
pub fn jobs(previous: bool) -> crate::Result<serde_json::Value> {
    let paths = basecraft_platform::paths::AppPaths::discover()
        .map_err(|e| crate::error::invalid(e.to_string()))?;
    job_records(
        &paths.state.join("jobs"),
        previous,
        basecraft_platform::process_instance(),
    )
}

fn job_records(directory: &Path, previous: bool, owner: &str) -> crate::Result<serde_json::Value> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({"jobs":[]}));
        }
        Err(e) => return Err(e.into()),
    };
    let mut runs = Vec::new();
    let mut total_bytes = 0u64;
    let mut warnings = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 4096 {
            warnings.push("Showing at most 4096 retained runs".into());
            break;
        }
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("manifest.json");
        total_bytes = total_bytes.saturating_add(std::fs::metadata(&path).map_or(0, |m| m.len()));
        if total_bytes > 64 << 20 {
            warnings.push("Retained task display reached its 64 MiB reading limit. Individual manifests remain available in application state.".into());
            break;
        }
        let result = crate::provenance::read_record(&path)
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(Into::into));
        match result {
            Ok(mut run) => {
                if !run.is_object()
                    || run["schema_version"] != 1
                    || !run["id"].is_string()
                    || !run["status"].is_string()
                    || !run["operation"].is_string()
                    || !run["source"]["path"].is_string()
                    || !run["outputs"].is_object()
                {
                    warnings.push(format!(
                        "Unsupported or malformed task record: {}",
                        path.display()
                    ));
                    continue;
                }
                let current = run["owner_instance"].as_str() == Some(owner);
                if !previous && !current {
                    continue;
                }
                if run["status"] == "running" && !current {
                    // A PID alone is insufficient to prove a previous owner is still alive.
                    run["status"] = serde_json::json!("Previous owner; completion unconfirmed");
                }
                if let Some(object) = run.as_object_mut() {
                    object.remove("source_record");
                }
                run["record_path"] = serde_json::json!(path);
                runs.push(run);
            }
            Err(e) => warnings.push(format!("Could not read {}: {e}", path.display())),
        }
    }
    runs.sort_by(|a, b| b["started"].as_str().cmp(&a["started"].as_str()));
    Ok(serde_json::json!({"jobs":runs, "warnings":warnings}))
}

fn tool_methods(
    plugin: &str,
    operation: &str,
    params: &serde_json::Value,
    event: Option<&LogEntry>,
) -> String {
    let tool = match event.map(|e| &e.actor) {
        Some(Actor::Tool { tool, version }) => format!("{tool} (version {version})"),
        Some(Actor::Plugin { id, version }) => {
            format!("plugin {id} (version {version}; external tool version not recorded)")
        }
        _ => format!("{plugin} (software version not recorded)"),
    };
    let procedure = params
        .pointer("/_execution/methods/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("A recorded analysis was performed");
    let mut text = format!("{procedure} using {tool} (operation: {operation}).");
    if let Some(steps) = params
        .pointer("/_execution/preprocessing")
        .and_then(serde_json::Value::as_array)
    {
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            let _ = write!(text, " {step}.");
        }
    }
    if let Some(settings) = params
        .pointer("/_execution/resolved_parameters")
        .and_then(serde_json::Value::as_object)
    {
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
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_records_handle_missing_malformed_and_previous_owners() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = dir.path().join("jobs");
        assert_eq!(
            job_records(&jobs, false, "current").unwrap()["jobs"],
            serde_json::json!([])
        );
        assert!(!jobs.exists());
        for (id, value) in [
            ("broken", serde_json::json!([])),
            (
                "current",
                serde_json::json!({"schema_version":1,"id":"current","status":"running","operation":"align","source":{"path":"a.fasta"},"outputs":{},"owner_instance":"current"}),
            ),
            (
                "previous",
                serde_json::json!({"schema_version":1,"id":"previous","status":"running","operation":"align","source":{"path":"a.fasta"},"outputs":{},"owner_instance":"previous"}),
            ),
        ] {
            std::fs::create_dir_all(jobs.join(id)).unwrap();
            std::fs::write(jobs.join(id).join("manifest.json"), value.to_string()).unwrap();
        }
        let current = job_records(&jobs, false, "current").unwrap();
        assert_eq!(current["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(current["jobs"][0]["status"], "running");
        assert_eq!(current["warnings"].as_array().unwrap().len(), 1);
        let all = job_records(&jobs, true, "current").unwrap();
        assert_eq!(all["jobs"].as_array().unwrap().len(), 2);
        assert!(
            all["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|j| j["id"] == "previous"
                    && j["status"] == "Previous owner; completion unconfirmed")
        );
    }
}
