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
    /// Complete an operation's existing inventory with the submitted input's
    /// chronological history and producing activity. Retry preserves user Methods.
    pub fn publish_operation_activity(
        &self,
        run: &crate::analysis_store::ArtifactRun,
        activity: &LogEntry,
    ) -> crate::Result<()> {
        let manifest = run.directory().join("manifest.json");
        let mut publication: Publication =
            serde_json::from_slice(&crate::provenance::read_record(&manifest)?)?;
        let (events, effective) = crate::session::portable_analysis_activity(self, activity);
        publication.events = events;
        publication.effective_edits = effective;
        // Preserve upstream bundles used by the submitted revision for packaging.
        let mut input = crate::input_evidence::read_input(run.directory())?;
        let mut upstream = input
            .extra
            .get("upstream")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (bundle, origin) in &self.origins {
            let mut bytes = serde_json::to_vec_pretty(origin)?;
            bytes.push(b'\n');
            let reference = serde_json::json!({"bundle":crate::input_evidence::relative(run.directory(), &bundle.canonicalize()?)?,"id":origin.id,"hash":basecraft_io::hash_bytes(&bytes)});
            if !upstream.contains(&reference) {
                upstream.push(reference);
            }
        }
        if !upstream.is_empty() {
            input
                .extra
                .insert("upstream".into(), serde_json::json!(upstream));
            crate::input_evidence::write_json(&run.directory().join("input.json"), &input)?;
        }
        crate::provenance::publish_operation(run.directory(), &publication)
    }

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
    /// shows it. Metadata and submission JSON use their own readers. Other
    /// documents open through [`Self::from_file`].
    pub fn saved_report(path: &Path) -> crate::Result<Report> {
        if crate::metadata::is_manifest(path) || crate::submission::is_submission(path) {
            return Self::from_file(path)?.report();
        }
        if let Some(review) = crate::sanger_review::saved_review(path) {
            let review = review.map_err(crate::error::invalid)?;
            // A checkpoint recovered from application state is unsaved work.
            let mut report = review
                .document_report(review.recovery_path().is_some())
                .map_err(crate::error::invalid)?;
            // Keep the older biological audit and include new shared-journal decisions.
            let mut shared_record = Self::from_file(path)?;
            shared_record
                .events
                .retain(|event| !matches!(event.op, Op::Open { .. }));
            let shared = shared_record.report()?;
            report.warnings.extend(shared.warnings);
            if !shared.history.is_empty() {
                report.history.extend(shared.history);
                report.history_text = report
                    .history
                    .iter()
                    .map(|row| row.summary.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            return Ok(report);
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
        crate::contributions::project(&record.events, &mut record.effective);
        if record.events.iter().any(|event| matches!(&event.op,
            Op::ToolRun { outputs, params, .. } if outputs.is_empty() && params.get("contributions").is_none())) {
            record.warnings.push("Retained contribution ownership is unknown for legacy no-output activities; historical invocation evidence was not reassigned to current edits.".into());
        }
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
            .map(|(i, p)| {
                serde_json::to_value(basecraft_protocol::report_records::ReportSourceMapEntry {
                    paragraph: i + 1,
                    activities: p.activity_ids.clone(),
                })
            })
            .collect::<serde_json::Result<Vec<_>>>()?;
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
        let manifest =
            serde_json::to_value(basecraft_protocol::report_records::ReportExportManifest {
                schema_version: 1,
                revision: self.revision.clone(),
                document: self.document.clone(),
                unsaved: self.unsaved,
                template_version: self.template_version,
                coordinates: "raw zero-based half-open; summaries one-based".into(),
            })?;
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
    let mut budget = TaskReadBudget(64 << 20);
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
        let result = budget
            .read(&path)
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
                if run.get("runtime_record").is_some()
                    && let Err(error) = reconcile_task_receipt(&mut run, &mut budget)
                {
                    let notice = format!(
                        "Canonical task receipt unavailable or unsupported for {}: {error}",
                        path.display()
                    );
                    run["status"] =
                        serde_json::json!("Completion unconfirmed; receipt unavailable");
                    run["outputs"] = serde_json::json!({});
                    run["artifacts"] = serde_json::json!([]);
                    run["errors"] = serde_json::json!([]);
                    run["warnings"] = serde_json::json!([]);
                    run["effective_parameters"] = serde_json::Value::Null;
                    run["primary_output"] = serde_json::Value::Null;
                    run["diagnostics"] = serde_json::json!(notice);
                    run["receipt_confirmed"] = serde_json::json!(false);
                    warnings.push(notice);
                }
                if matches!(
                    run["status"].as_str(),
                    Some("queued" | "running" | "cancelling" | "finalizing")
                ) && !current
                {
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
        if budget.0 == 0 {
            warnings.push("Retained task display reached its 64 MiB reading limit, including canonical receipts. Evidence remains available at its recorded location.".into());
            break;
        }
    }
    runs.sort_by(|a, b| b["started"].as_str().cmp(&a["started"].as_str()));
    Ok(serde_json::json!({"jobs":runs, "warnings":warnings}))
}

/// Charge both discovery and canonical evidence, including malformed JSON. The
/// bounded reader also caps a file that grows after metadata was inspected.
struct TaskReadBudget(u64);

impl TaskReadBudget {
    fn read(&mut self, path: &Path) -> crate::Result<Vec<u8>> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(crate::error::invalid("evidence is not a regular file"));
        }
        let size = metadata.len();
        if size > self.0 {
            self.0 = 0;
            return Err(crate::error::invalid(
                "retained task aggregate reading limit (64 MiB) exceeded",
            ));
        }
        crate::provenance::read_record_with_budget(path, &mut self.0)
    }
}

fn reconcile_task_receipt(
    run: &mut serde_json::Value,
    budget: &mut TaskReadBudget,
) -> crate::Result<()> {
    use basecraft_protocol::operation::{CONTRACT_VERSION, OperationOutcome};
    let path = run["runtime_record"]
        .as_str()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| crate::error::invalid("invalid runtime receipt pointer"))?;
    // Registered writers store an absolute pointer. A moved project requires
    // explicit relinking; listing never searches for or rewrites its evidence.
    if !Path::new(path).is_absolute() {
        return Err(crate::error::invalid(
            "runtime receipt pointer must be absolute",
        ));
    }
    let outcome: OperationOutcome = serde_json::from_slice(&budget.read(Path::new(path))?)?;
    if outcome.schema != CONTRACT_VERSION {
        return Err(crate::error::invalid(
            "unsupported operation receipt schema",
        ));
    }
    if run["id"] != outcome.run_id || run["operation"] != outcome.operation {
        return Err(crate::error::invalid("operation receipt identity mismatch"));
    }
    let primary = task_primary_output(&outcome, Path::new(path), budget);
    run["status"] = serde_json::to_value(outcome.state)?;
    run["effective_parameters"] = outcome.effective_parameters;
    run["outputs"] = serde_json::to_value(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), artifact.digest.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
    )?;
    run["artifacts"] = serde_json::to_value(&outcome.artifacts)?;
    run["primary_output"] =
        serde_json::json!(primary.as_ref().ok().and_then(|path| path.as_deref()));
    run["diagnostics"] = serde_json::json!(
        outcome
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .chain(outcome.warnings.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    );
    run["errors"] = serde_json::to_value(outcome.errors)?;
    run["warnings"] = serde_json::to_value(outcome.warnings)?;
    if let Err(error) = primary {
        let notice = format!("Primary result unavailable: {error}");
        run["diagnostics"] = serde_json::json!(
            format!(
                "{}\n{notice}",
                run["diagnostics"].as_str().unwrap_or_default()
            )
            .trim()
        );
    }
    run["receipt_confirmed"] = serde_json::json!(true);
    Ok(())
}

/// Publication outputs are ordered by the runner with exported results first.
/// Only offer a document that is BOTH a canonical published artifact and a
/// visible publication output, never an arbitrary retained support member.
fn task_primary_output(
    outcome: &basecraft_protocol::operation::OperationOutcome,
    runtime: &Path,
    budget: &mut TaskReadBudget,
) -> crate::Result<Option<String>> {
    if !outcome.state.terminal() || outcome.artifacts.is_empty() {
        return Ok(None);
    }
    let root = runtime
        .parent()
        .ok_or_else(|| crate::error::invalid("receipt has no directory"))?;
    let publication: Publication =
        serde_json::from_slice(&budget.read(&root.join("manifest.json"))?)?;
    if publication.schema_version != 1 || publication.id != outcome.run_id {
        return Err(crate::error::invalid(
            "publication schema or run identity mismatch",
        ));
    }
    // Use the accepted descriptor's exact primary role/member, not collection
    // ordering. A result collection can include primary and supporting trees.
    let descriptor: basecraft_protocol::operation::OperationDescriptor =
        serde_json::from_slice(&budget.read(&root.join("descriptor.json"))?)?;
    if basecraft_io::hash_bytes(&serde_json::to_vec(&descriptor)?) != outcome.pins.descriptor_digest
    {
        return Err(crate::error::invalid(
            "primary result descriptor identity mismatch",
        ));
    }
    let primary = descriptor
        .legacy_adapter
        .as_ref()
        .and_then(|adapter| adapter.outputs.get("output"));
    for retained_only in [false, true] {
        for output in &publication.outputs {
            let logical = Path::new(&output.logical_name);
            if !normal_task_path(logical) || crate::files::Format::from_path(logical).is_err() {
                continue;
            }
            if let Some(artifact) = outcome.artifacts.iter().find(|artifact| {
                use basecraft_protocol::operation::ArtifactVisibility;
                let path = Path::new(&artifact.path);
                if artifact.digest != output.hash || !path.is_absolute() {
                    return false;
                }
                let binding = descriptor.outputs.get(&artifact.role);
                let result_role =
                    binding.is_some_and(|binding| binding.visibility == ArtifactVisibility::Result);
                let visible = !path.starts_with(root)
                    && path.ends_with(logical)
                    && !logical
                        .components()
                        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'));
                let retained = output
                    .retained
                    .as_deref()
                    .map(Path::new)
                    .filter(|path| normal_task_path(path));
                let matches_primary = primary.is_some_and(|primary| {
                    if primary.role != artifact.role || !normal_task_path(Path::new(&primary.path))
                    {
                        return false;
                    }
                    let expected = if binding.is_some_and(|binding| binding.collection) {
                        Path::new(&primary.role).join(&primary.path)
                    } else {
                        Path::new(&primary.role).to_path_buf()
                    };
                    retained.is_some_and(|retained| retained == expected)
                });
                if !retained_only {
                    return visible
                        && (artifact.role == "workspace"
                            || (result_role && (primary.is_none() || matches_primary)));
                }
                result_role
                    && matches_primary
                    && outcome.state == basecraft_protocol::operation::RuntimeState::Completed
                    && retained.is_some_and(|retained| root.join(retained) == path)
            }) {
                return Ok(Some(artifact.path.clone()));
            }
        }
    }
    Ok(None)
}

fn normal_task_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
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
        .or_else(|| params.get("effective"))
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
    if operation == "import_support" {
        let _ = write!(
            text,
            " Support mapping: matching clades; source {}; sets {}; matched branches {}.",
            params["source"], params["sets"], params["matched"]
        );
    }
    if let Some(event) = event {
        for (name, hash) in &event.result.hashes {
            let _ = write!(text, " Input evidence: {name} ({hash}).");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered_fixture(
        root: &Path,
        state: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf, serde_json::Value) {
        let jobs = root.join("jobs");
        let receipt = root.join("run/runtime.json");
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        std::fs::create_dir_all(jobs.join("registered")).unwrap();
        let outcome = serde_json::json!({"schema":1,"run_id":"registered","operation":"test/run@1","state":state,
            "pins":{"generation":"g","package_digest":"p","descriptor_digest":"d"},
            "requested_parameters":{},"effective_parameters":{"canonical":true},"data":null,
            "artifacts":[],"proposals":null,"warnings":["canonical warning"],
            "errors":[{"code":"tool_failure","message":"canonical error","operation":"test/run@1"}],"evidence":null});
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        let index = serde_json::json!({"schema_version":1,"id":"registered","status":"completed","operation":"test/run@1",
            "owner_instance":"current","source":{"path":"input.afa"},"outputs":{"stale.nwk":"stale"},
            "primary_output":"stale.nwk","diagnostics":"stale error","runtime_record":receipt});
        std::fs::write(jobs.join("registered/manifest.json"), index.to_string()).unwrap();
        (jobs, receipt, outcome)
    }

    #[test]
    fn task_receipts_override_stale_index_for_every_state_and_previous_owner() {
        for state in [
            "queued",
            "running",
            "cancelling",
            "finalizing",
            "completed",
            "partial",
            "failed",
            "cancelled",
            "interrupted",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (jobs, receipt, outcome) = registered_fixture(temp.path(), state);
            let index = jobs.join("registered/manifest.json");
            let before = std::fs::read(&index).unwrap();
            let rows = job_records(&jobs, false, "current").unwrap();
            let row = &rows["jobs"][0];
            assert_eq!(row["status"], state);
            assert_eq!(row["outputs"], serde_json::json!({}));
            assert!(row["primary_output"].is_null());
            assert_eq!(row["diagnostics"], "canonical error\ncanonical warning");
            assert_eq!(row["effective_parameters"]["canonical"], true);
            assert_eq!(row["receipt_confirmed"], true);
            assert_eq!(
                job_records(&jobs, false, "new-owner").unwrap()["jobs"],
                serde_json::json!([])
            );
            let previous = job_records(&jobs, true, "new-owner").unwrap();
            if ["queued", "running", "cancelling", "finalizing"].contains(&state) {
                assert_eq!(
                    previous["jobs"][0]["status"],
                    "Previous owner; completion unconfirmed"
                );
            } else {
                assert_eq!(previous["jobs"][0]["status"], state);
            }
            assert_eq!(std::fs::read(index).unwrap(), before);
            assert_eq!(
                std::fs::read_to_string(receipt).unwrap(),
                outcome.to_string()
            );
        }
    }

    #[test]
    fn task_receipts_refuse_missing_moved_corrupt_future_and_mismatched_evidence() {
        for damage in [
            "missing",
            "moved",
            "corrupt",
            "future",
            "run",
            "operation",
            "bad-pointer",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
            match damage {
                "missing" => std::fs::remove_file(&receipt).unwrap(),
                "moved" => std::fs::rename(&receipt, receipt.with_extension("moved")).unwrap(),
                "corrupt" => std::fs::write(&receipt, "{\"schema\":1,").unwrap(),
                "bad-pointer" => {
                    let index = jobs.join("registered/manifest.json");
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
                    value["runtime_record"] = serde_json::json!(42);
                    std::fs::write(index, value.to_string()).unwrap();
                }
                field => {
                    match field {
                        "future" => outcome["schema"] = serde_json::json!(999),
                        "run" => outcome["run_id"] = serde_json::json!("other"),
                        _ => outcome["operation"] = serde_json::json!("other"),
                    }
                    std::fs::write(&receipt, outcome.to_string()).unwrap();
                }
            }
            let rows = job_records(&jobs, false, "current").unwrap();
            let row = &rows["jobs"][0];
            assert_eq!(
                row["status"], "Completion unconfirmed; receipt unavailable",
                "{damage}"
            );
            assert_eq!(row["receipt_confirmed"], false);
            assert!(row["primary_output"].is_null());
            assert_eq!(row["outputs"], serde_json::json!({}));
            assert_eq!(rows["warnings"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn task_primary_result_uses_real_distance_primary_member_not_bootstrap_order() {
        use basecraft_protocol::operation::{ArtifactVisibility, OperationContract};
        let temp = tempfile::tempdir().unwrap();
        let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
        let root = receipt.parent().unwrap();
        let tree = root.join("artifacts/tree.nwk");
        let bootstrap = root.join("artifacts/bootstrap-run.partial.nex");
        let contract: OperationContract =
            serde_json::from_slice(include_bytes!("../../../plugins/distance/operations.json"))
                .unwrap();
        let mut descriptor = contract
            .operations
            .into_iter()
            .find(|descriptor| descriptor.name == "tree")
            .unwrap();
        let mut publication = Publication {
            schema_version: 1,
            id: "registered".into(),
            session: "current".into(),
            parents: vec![],
            parent_hashes: std::collections::BTreeMap::default(),
            created: String::new(),
            origin: String::new(),
            inputs: vec![],
            outputs: [
                ("artifacts/bootstrap-run.partial.nex", "support"),
                ("artifacts/tree.nwk", "canonical"),
            ]
            .into_iter()
            .map(
                |(member, hash)| basecraft_protocol::provenance::ArtifactRevision {
                    id: member.into(),
                    logical_name: format!(".basecraft/run/{member}"),
                    hash: hash.into(),
                    size: 0,
                    retained: Some(member.into()),
                    reference: None,
                },
            )
            .collect(),
            events: vec![],
            effective_edits: vec![],
            completeness: basecraft_protocol::provenance::Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: true,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        // Actual SDK ordering: bootstrap first; both members have the SAME
        // artifacts/result role, including withheld partial bootstrap support.
        outcome["artifacts"] = serde_json::json!([
            {"role":"artifacts","path":bootstrap,"digest":"support"},
            {"role":"artifacts","path":tree,"digest":"canonical"}]);
        outcome["pins"]["descriptor_digest"] = serde_json::json!(basecraft_io::hash_bytes(
            &serde_json::to_vec(&descriptor).unwrap()
        ));
        let descriptor_path = root.join("descriptor.json");
        let manifest = root.join("manifest.json");
        std::fs::write(&descriptor_path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
        std::fs::write(&manifest, serde_json::to_vec(&publication).unwrap()).unwrap();
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert_eq!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        outcome["artifacts"][0]["path"] =
            serde_json::json!(root.join("artifacts/bootstrap-run.nex"));
        publication.outputs[0].logical_name = ".basecraft/run/artifacts/bootstrap-run.nex".into();
        publication.outputs[0].retained = Some("artifacts/bootstrap-run.nex".into());
        std::fs::write(&manifest, serde_json::to_vec(&publication).unwrap()).unwrap();
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert_eq!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        outcome["state"] = serde_json::json!("finalizing");
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
        outcome["state"] = serde_json::json!("completed");
        // A canonical visible export of that exact primary member wins.
        let export = temp.path().join("inferred.nwk");
        publication.outputs.insert(
            0,
            basecraft_protocol::provenance::ArtifactRevision {
                id: "export".into(),
                logical_name: "inferred.nwk".into(),
                hash: "canonical".into(),
                size: 0,
                retained: Some("artifacts/tree.nwk".into()),
                reference: None,
            },
        );
        outcome["artifacts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"role":"artifacts","path":export,"digest":"canonical"}));
        std::fs::write(&manifest, serde_json::to_vec(&publication).unwrap()).unwrap();
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert_eq!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"],
            export.to_str().unwrap()
        );
        // Stale export digest cannot override the valid retained primary.
        outcome["artifacts"][2]["digest"] = serde_json::json!("changed");
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert_eq!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"],
            tree.to_str().unwrap()
        );
        // Missing primary must not substitute the bootstrap collection.
        outcome["artifacts"].as_array_mut().unwrap().remove(1);
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
        // A genuinely supporting-only descriptor never offers a primary.
        descriptor.outputs.get_mut("artifacts").unwrap().visibility =
            ArtifactVisibility::Supporting;
        outcome["artifacts"] =
            serde_json::json!([{ "role":"artifacts","path":tree,"digest":"canonical"}]);
        outcome["pins"]["descriptor_digest"] = serde_json::json!(basecraft_io::hash_bytes(
            &serde_json::to_vec(&descriptor).unwrap()
        ));
        std::fs::write(&descriptor_path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
        std::fs::write(&receipt, outcome.to_string()).unwrap();
        assert!(
            job_records(&jobs, false, "current").unwrap()["jobs"][0]["primary_output"].is_null()
        );
        descriptor.outputs.get_mut("artifacts").unwrap().visibility = ArtifactVisibility::Result;
        std::fs::write(&descriptor_path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
        let row = &job_records(&jobs, false, "current").unwrap()["jobs"][0];
        assert!(row["primary_output"].is_null());
        assert!(
            row["diagnostics"]
                .as_str()
                .unwrap()
                .contains("descriptor identity mismatch")
        );
        assert_eq!(row["status"], "completed");
    }

    #[cfg(unix)]
    #[test]
    fn task_nonregular_evidence_is_rejected_in_bounded_subprocess() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD: &str = "BASECRAFT_TASK_FIFO_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            let rows = job_records(&Path::new(&root).join("jobs"), false, "current").unwrap();
            let text = rows.to_string();
            assert!(text.contains("not a regular file"), "{text}");
            assert!(!text.contains("reading limit"), "{text}");
            assert!(
                rows["jobs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == "regular")
            );
            return;
        }
        for location in ["index", "runtime", "publication"] {
            let temp = tempfile::tempdir().unwrap();
            let (jobs, receipt, mut outcome) = registered_fixture(temp.path(), "completed");
            let path = match location {
                "index" => jobs.join("registered/manifest.json"),
                "runtime" => receipt.clone(),
                _ => {
                    outcome["artifacts"] = serde_json::json!([{ "role":"artifacts","path":"/tree.nwk","digest":"hash"}]);
                    std::fs::write(&receipt, outcome.to_string()).unwrap();
                    receipt.parent().unwrap().join("manifest.json")
                }
            };
            if path.exists() {
                std::fs::remove_file(&path).unwrap();
            }
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
            std::fs::create_dir_all(jobs.join("regular")).unwrap();
            std::fs::write(jobs.join("regular/manifest.json"), serde_json::json!({"schema_version":1,"id":"regular","operation":"legacy","status":"completed","source":{"path":"input"},"outputs":{},"owner_instance":"current"}).to_string()).unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "document_record::tests::task_nonregular_evidence_is_rejected_in_bounded_subprocess", "--nocapture"])
                .env(CHILD, temp.path()).spawn().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "{location}");
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("{location} FIFO hung the retained Tasks query");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let regular = temp.path().join("regular.json");
        let link = temp.path().join("regular-link.json");
        std::fs::write(&regular, "{}").unwrap();
        std::os::unix::fs::symlink(&regular, &link).unwrap();
        assert_eq!(crate::provenance::read_record(&link).unwrap(), b"{}");
        let mut budget = TaskReadBudget(1024);
        assert!(budget.read(temp.path()).is_err());
        assert_eq!(budget.0, 1024);
        let mut permissions = std::fs::metadata(&regular).unwrap().permissions();
        permissions.set_mode(0o0);
        std::fs::set_permissions(&regular, permissions).unwrap();
        // Root may bypass permissions; normal validation runs as an unprivileged user.
        if std::fs::File::open(&regular).is_err() {
            assert!(budget.read(&regular).is_err());
            assert_eq!(budget.0, 1024);
        }
    }

    #[test]
    fn registered_legacy_and_sanger_tasks_coexist_without_listing_writes() {
        let temp = tempfile::tempdir().unwrap();
        let (jobs, _, _) = registered_fixture(temp.path(), "partial");
        for (id, primary) in [
            ("legacy", serde_json::Value::Null),
            ("sanger", serde_json::json!("called.fa")),
        ] {
            let directory = jobs.join(id);
            std::fs::create_dir_all(&directory).unwrap();
            let value = serde_json::json!({"schema_version":1,"id":id,"status":"completed","operation":id,
                "source":{"path":"input"},"outputs":{},"primary_output":primary,"owner_instance":"current"});
            std::fs::write(directory.join("manifest.json"), value.to_string()).unwrap();
        }
        let first = job_records(&jobs, false, "current").unwrap();
        let second = job_records(&jobs, true, "current").unwrap();
        assert_eq!(first, second);
        let rows = first["jobs"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().filter(|row| row["id"] == "registered").count(),
            1
        );
        assert_eq!(
            rows.iter().find(|row| row["id"] == "sanger").unwrap()["primary_output"],
            "called.fa"
        );
        assert_eq!(std::fs::read_dir(&jobs).unwrap().count(), 3);
        assert_eq!(job_records(&jobs, false, "current").unwrap(), first);
    }

    #[test]
    fn task_read_budget_includes_receipts_and_caps_each_file() {
        let temp = tempfile::tempdir().unwrap();
        let (jobs, receipt, _) = registered_fixture(temp.path(), "completed");
        let index = jobs.join("registered/manifest.json");
        let size = std::fs::metadata(&index).unwrap().len();
        let mut budget = TaskReadBudget(size + 1);
        budget.read(&index).unwrap();
        assert!(budget.read(&receipt).is_err());
        assert_eq!(budget.0, 0);
        let file = std::fs::File::create(&receipt).unwrap();
        file.set_len((64 << 20) + 1).unwrap();
        assert!(crate::provenance::read_record(&receipt).is_err());
        let rows = job_records(&jobs, false, "current").unwrap();
        assert_eq!(rows["jobs"][0]["receipt_confirmed"], false);
        assert!(
            rows["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("64 MiB"))
        );
    }

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
