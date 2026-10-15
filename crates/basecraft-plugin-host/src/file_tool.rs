//! Plugin operations that read and write files, such as aligners.
//!
//! A run happens in three steps, so the slow middle one can go to another
//! thread: [`FileJob::from_alignment`] writes the tool's input from an open
//! document, [`FileJob::run`] runs the plugin, and [`FileJob::record`] logs
//! a `tool.run` operation with the hashes of what went in and came out.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use basecraft_ops::files::{HashingWriter, write_alignment, write_ungapped};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::Actor;
use basecraft_protocol::plugin::InvokeResult;
use serde_json::json;

use crate::{Error, Manifest, PluginProcess, Result};

static CANCELLATIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, PathBuf>>,
> = std::sync::OnceLock::new();

fn cancellations() -> &'static std::sync::Mutex<std::collections::HashMap<String, PathBuf>> {
    CANCELLATIONS.get_or_init(std::sync::Mutex::default)
}

pub fn can_cancel_run(id: &str) -> bool {
    cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(id)
}

/// Only an active in-process job can authorize a cancellation file write.
pub fn cancel_run(id: &str) -> Result<()> {
    let path = cancellations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .cloned()
        .ok_or_else(|| Error::Protocol("Cancellation is unavailable for this run".into()))?;
    basecraft_io::write_atomic(&path, |w| w.write_all(b"cancel\n"))
        .map_err(basecraft_ops::Error::from)?;
    Ok(())
}

struct CancellationRegistration(String);
impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        cancellations()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// What a tool gets from an alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignmentInput {
    /// The rows without gaps, for an aligner.
    Ungapped,
    /// The alignment as it is, for tree building.
    Aligned,
}

pub struct FileJob {
    /// The plugin's directory and the operation to call.
    pub plugin: PathBuf,
    pub operation: String,
    pub input: PathBuf,
    /// What the input was made from, with the hash of the input itself.
    pub input_key: (String, String),
    pub output: PathBuf,
    /// Public operation parameters, retained in the tool-run log.
    pub params: serde_json::Map<String, serde_json::Value>,
    run_id: String,
    source_record: basecraft_ops::document_record::DocumentRecord,
    cpu_budget: usize,
    granted_budget: std::sync::atomic::AtomicUsize,
    source_path: PathBuf,
    source_version: u64,
    source_hash: String,
    _dir: tempfile::TempDir,
}

impl FileJob {
    /// A job whose input is the alignment as FASTA, with or without gaps.
    pub fn from_alignment(
        session: &Session,
        doc: DocId,
        (plugin, operation): (&Path, &str),
        kind: AlignmentInput,
        output: PathBuf,
    ) -> Result<Self> {
        let source_path = session.root().join(session.doc_key(doc));
        if output.exists() && output.canonicalize()? == source_path.canonicalize()? {
            return Err(Error::Protocol(
                "tool output must differ from the source alignment".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-tool")
            .tempdir()?;
        let path = dir.path().join("input.fasta");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut hw = HashingWriter::new(&mut file);
        let key = match kind {
            AlignmentInput::Ungapped => {
                write_ungapped(session.alignment(doc), &mut hw)?;
                format!("{} (ungapped)", session.doc_key(doc))
            }
            AlignmentInput::Aligned => {
                write_alignment(session.alignment(doc), &mut hw)?;
                session.doc_key(doc).to_string()
            }
        };
        let hash = hw.hash();
        file.flush()?;
        let run_id = ulid::Ulid::generate().to_string();
        Ok(Self {
            plugin: plugin.to_path_buf(),
            operation: operation.to_string(),
            input: path,
            input_key: (key, hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: session.root().join(session.doc_key(doc)),
            source_version: session.generation(doc),
            source_record: session.document_record(doc)?,
            source_hash: basecraft_io::hash_file(&session.root().join(session.doc_key(doc)))
                .map_err(basecraft_ops::Error::from)?,
            cpu_budget: crate::compute::effective_allocation()?,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            _dir: dir,
        })
    }

    /// Run the plugin. Touches no session, so it can run on any thread.
    pub fn from_tree(
        session: &Session,
        doc: DocId,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        if !session.is_tree(doc) {
            return Err(Error::Protocol("a tree input is required".into()));
        }
        let source_path = session.root().join(session.doc_key(doc));
        if output.exists() && output.canonicalize()? == source_path.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from source tree".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-tree-job-")
            .tempdir()?;
        let input = dir.path().join("input.nwk");
        std::fs::write(&input, session.tree(doc).to_newick())?;
        let hash = basecraft_io::hash_file(&input).map_err(basecraft_ops::Error::from)?;
        let run_id = ulid::Ulid::generate().to_string();
        Ok(Self {
            plugin: plugin.to_path_buf(),
            operation: operation.into(),
            input,
            input_key: (session.doc_key(doc).into(), hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: source_path.clone(),
            source_version: session.generation(doc),
            source_record: session.document_record(doc)?,
            source_hash: basecraft_io::hash_file(&source_path)
                .map_err(basecraft_ops::Error::from)?,
            cpu_budget: crate::compute::effective_allocation()?,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            _dir: dir,
        })
    }

    /// Public immutable file input for analyses coordinated by another document.
    pub fn from_file(
        session: &Session,
        doc: DocId,
        input: &Path,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        let source_path = session.root().join(session.doc_key(doc));
        if output.exists() && output.canonicalize()? == input.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from analysis input".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-file-job-")
            .tempdir()?;
        let snapshot = dir.path().join(
            input
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("input")),
        );
        std::fs::copy(input, &snapshot)?;
        let hash = basecraft_io::hash_file(&snapshot).map_err(basecraft_ops::Error::from)?;
        let run_id = ulid::Ulid::generate().to_string();
        Ok(Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            source_version: session.generation(doc),
            source_record: basecraft_ops::document_record::DocumentRecord::from_file(input)?,
            source_hash: basecraft_io::hash_file(&source_path)
                .map_err(basecraft_ops::Error::from)?,
            source_path,
            run_id,
            cpu_budget: crate::compute::effective_allocation()?,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            _dir: dir,
        })
    }

    /// Compose a dependent public file operation without accessing a document on
    /// the worker thread. The next input is copied before the stage starts.
    pub fn following(
        &self,
        input: &Path,
        (plugin, operation): (&Path, &str),
        output: PathBuf,
    ) -> Result<Self> {
        if output.exists() && output.canonicalize()? == input.canonicalize()? {
            return Err(Error::Protocol(
                "output must differ from analysis input".into(),
            ));
        }
        let dir = tempfile::Builder::new()
            .prefix("basecraft-following-job-")
            .tempdir()?;
        let snapshot = dir.path().join(
            input
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("input")),
        );
        std::fs::copy(input, &snapshot)?;
        let hash = basecraft_io::hash_file(&snapshot).map_err(basecraft_ops::Error::from)?;
        let run_id = ulid::Ulid::generate().to_string();
        Ok(Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash),
            output,
            params: [("job_id".into(), json!(run_id.clone()))]
                .into_iter()
                .collect(),
            run_id,
            source_path: self.source_path.clone(),
            source_version: self.source_version,
            source_record: self.following_record(input)?,
            source_hash: self.source_hash.clone(),
            cpu_budget: self.cpu_budget,
            granted_budget: std::sync::atomic::AtomicUsize::new(0),
            _dir: dir,
        })
    }

    fn following_record(
        &self,
        input: &Path,
    ) -> Result<basecraft_ops::document_record::DocumentRecord> {
        use basecraft_protocol::{LogEntry, Op, OpResult, provenance::EditActivity};
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let manifest_path = paths
            .state
            .join("jobs")
            .join(&self.run_id)
            .join("manifest.json");
        let file = std::fs::File::open(&manifest_path)?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(file, 16 * 1024 * 1024 + 1),
            &mut bytes,
        )?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(Error::Protocol(
                "Retained execution record exceeds 16 MiB".into(),
            ));
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| Error::Protocol(e.to_string()))?;
        let hash = basecraft_io::hash_file(input).map_err(basecraft_ops::Error::from)?;
        let key = input.display().to_string();
        if manifest["status"] != "completed"
            || manifest["outputs"][&key].as_str() != Some(hash.as_str())
        {
            // An intermediate conversion without matching execution evidence stays an unknown boundary.
            return Ok(basecraft_ops::document_record::DocumentRecord::from_file(
                input,
            )?);
        }
        let mut record = self.source_record.clone();
        let mut params = manifest["effective_parameters"].clone();
        if !params.is_object() {
            return Err(Error::Protocol(
                "Retained execution parameters must be an object".into(),
            ));
        }
        if !manifest["execution"].is_null() {
            params["_execution"] = manifest["execution"].clone();
        }
        let actor = if let Ok(tool) =
            serde_json::from_value::<basecraft_protocol::plugin::ToolInfo>(manifest["tool"].clone())
        {
            Actor::Tool {
                tool: tool.name,
                version: tool.version,
            }
        } else {
            Actor::Plugin {
                id: manifest["plugin"]["id"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                version: manifest["plugin"]["version"]
                    .as_str()
                    .unwrap_or("not recorded")
                    .into(),
            }
        };
        let mut input_session = Session::recoverable(
            input
                .parent()
                .ok_or_else(|| Error::Protocol("input has no parent".into()))?,
        )?;
        let input_doc = input_session.open(input)?;
        let op = Op::ToolRun {
            plugin: manifest["plugin"]["id"]
                .as_str()
                .unwrap_or("unknown")
                .into(),
            operation: self.operation.clone(),
            params,
            inputs: vec![self.input_key.0.clone()],
            outputs: vec![key.clone()],
        };
        record.events.push(LogEntry {
            v: 1,
            id: self.run_id.clone(),
            t: manifest["ended"].as_str().unwrap_or("").into(),
            host: String::new(),
            session: self.run_id.clone(),
            actor,
            doc: key.clone(),
            op: op.clone(),
            result: OpResult {
                hashes: vec![self.input_key.clone(), (key, hash)],
                feature: None,
            },
            state: input_session.state(input_doc),
        });
        record.effective.push(EditActivity {
            id: self.run_id.clone(),
            operation: op,
        });
        Ok(record)
    }

    fn execution_budget(&self) -> usize {
        let granted = self
            .granted_budget
            .load(std::sync::atomic::Ordering::Relaxed);
        if granted == 0 {
            self.cpu_budget
        } else {
            granted
        }
    }

    fn retained_input(&self) -> Result<PathBuf> {
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let id = &self.run_id;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(Error::Protocol("invalid run identity".into()));
        }
        let retained = paths.state.join("jobs").join(id).join("input.snapshot");
        let path = if retained.is_file() {
            retained
        } else {
            self.input.clone()
        };
        if basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)? != self.input_key.1 {
            return Err(Error::Protocol("submitted input snapshot changed".into()));
        }
        Ok(path)
    }

    /// Record a terminal outcome even if the plugin reports an RPC error.
    /// Input snapshots outlive the temporary worker directory through the store.
    pub fn record_terminal(
        &self,
        session: &mut Session,
        doc: DocId,
        outcome: &Result<InvokeResult>,
        elapsed_seconds: f64,
    ) -> Result<()> {
        use basecraft_protocol::delimitation::{JobRecord, JobStatus};
        session.snapshot_file(&self.retained_input()?)?;
        let cancelled = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|p| Path::new(p).exists());
        let diagnostics = outcome
            .as_ref()
            .err()
            .map_or_else(Vec::new, |e| vec![e.to_string()]);
        let mut report = JobRecord {
            id: self.run_id.clone(),
            generation: self.source_version,
            status: if outcome.is_ok() {
                JobStatus::Completed
            } else if cancelled {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            },
            method: self.operation.clone(),
            inputs: std::iter::once(self.input_key.clone()).collect(),
            outputs: std::collections::BTreeMap::new(),
            diagnostics,
            elapsed_seconds,
        };
        let path = PathBuf::from(format!("{}.run.json", self.output.display()));
        if path.exists() {
            report
                .outputs
                .insert(path.display().to_string(), session.snapshot_file(&path)?);
            if let Ok(value) = std::fs::read(&path).and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(std::io::Error::other)
            }) {
                if let Ok(native) = serde_json::from_value::<JobRecord>(value["job"].clone()) {
                    if native.id == self.run_id && native.method == self.operation {
                        report.status = native.status;
                        report.diagnostics.extend(native.diagnostics);
                        for path in native.outputs.keys() {
                            let path = Path::new(path);
                            if path.is_file() {
                                report.outputs.insert(
                                    path.display().to_string(),
                                    session.snapshot_file(path)?,
                                );
                            }
                        }
                    }
                }
            }
        }
        session.log_job_finished(doc, report)?;
        Ok(())
    }

    pub fn run(&self) -> Result<InvokeResult> {
        let paths = basecraft_platform::paths::AppPaths::discover()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let id = &self.run_id;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(Error::Protocol("invalid run identity".into()));
        }
        let run_dir = paths.state.join("jobs").join(id);
        // Unique ownership prevents reusing a run identity to overwrite evidence.
        std::fs::create_dir_all(paths.state.join("jobs"))?;
        std::fs::create_dir(&run_dir)?;
        let input = std::fs::read(&self.input)?;
        if basecraft_io::hash_bytes(&input) != self.input_key.1 {
            return Err(Error::Protocol("job input changed before launch".into()));
        }
        basecraft_io::write_atomic(&run_dir.join("input.snapshot"), |w| w.write_all(&input))
            .map_err(basecraft_ops::Error::from)?;
        let started = basecraft_platform::now_rfc3339();
        self.write_run_manifest(&run_dir, &started, "running", None)?;
        let _registration = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .map(|path| {
                cancellations()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(self.run_id.clone(), PathBuf::from(path));
                CancellationRegistration(self.run_id.clone())
            });
        let result = self.execute_run(&run_dir.join("input.snapshot"));
        let cancelled = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|p| Path::new(p).exists());
        let status = if result.is_ok() {
            "completed"
        } else if cancelled {
            "cancelled"
        } else {
            "failed"
        };
        self.write_run_manifest(&run_dir, &started, status, Some(&result))?;
        result
    }

    fn write_run_manifest(
        &self,
        directory: &Path,
        started: &str,
        status: &str,
        outcome: Option<&Result<InvokeResult>>,
    ) -> Result<()> {
        let manifest = Manifest::load(&self.plugin)?;
        let mut parameters =
            crate::compute::parameters(&manifest.id, self.params.clone(), self.execution_budget());
        parameters.insert("job_id".into(), json!(self.run_id));
        let mut outputs = serde_json::Map::new();
        if outcome.is_some_and(Result::is_ok) {
            let hash = basecraft_io::hash_file(&self.output).map_err(basecraft_ops::Error::from)?;
            outputs.insert(self.output.display().to_string(), json!(hash));
        }
        let source_path =
            serde_json::to_value(&self.source_path).map_err(|e| Error::Protocol(e.to_string()))?;
        let value = json!({
            "schema_version": 1, "id": self.run_id, "status": status,
            "owner_instance": basecraft_platform::process_instance(),
            "source_record": self.source_record,
            "started": started, "ended": outcome.map(|_| basecraft_platform::now_rfc3339()),
            "plugin": { "id": manifest.id, "version": manifest.version },
            "operation": self.operation, "effective_parameters": parameters,
            "source": { "path": source_path, "generation": self.source_version, "disk_hash": self.source_hash },
            "input": { "logical_name": self.input_key.0, "hash": self.input_key.1, "retained": "input.snapshot", "bytes": std::fs::metadata(&self.input)?.len() },
            "outputs": outputs, "requested_cpu_budget": self.cpu_budget, "cpu_budget": self.execution_budget(),
            "diagnostics": outcome.and_then(|r| r.as_ref().err()).map(ToString::to_string),
            "tool": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.tool.as_ref()),
            "execution": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.execution.as_ref()),
            "completeness": { "executable_argv_recorded": outcome.and_then(|r| r.as_ref().ok()).and_then(|r| r.execution.as_ref()).is_some_and(|e| !e.argv.is_empty()), "environment_lock_recorded": false },
            "interruption_policy": "a running record left by a terminated owner is interrupted, never completed"
        });
        let mut bytes =
            serde_json::to_vec_pretty(&value).map_err(|e| Error::Protocol(e.to_string()))?;
        bytes.push(b'\n');
        basecraft_io::write_atomic(&directory.join("manifest.json"), |w| w.write_all(&bytes))
            .map_err(basecraft_ops::Error::from)?;
        Ok(())
    }

    fn execute_run(&self, frozen_input: &Path) -> Result<InvokeResult> {
        let cancel = self
            .params
            .get("cancel_file")
            .and_then(serde_json::Value::as_str)
            .map(Path::new);
        let permit = crate::compute::reserve_cpu(self.cpu_budget, cancel)?;
        let granted_budget = permit.threads();
        self.granted_budget
            .store(granted_budget, std::sync::atomic::Ordering::Relaxed);
        let manifest = Manifest::load(&self.plugin)?;
        let mut process = PluginProcess::spawn_with_allocation(&manifest, granted_budget)?;
        let mut params =
            crate::compute::parameters(&manifest.id, self.params.clone(), granted_budget);
        params.insert("job_id".into(), json!(self.run_id));
        params.entry("original_source").or_insert(
            serde_json::to_value(&self.source_path).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        params.insert(
            "input".into(),
            serde_json::to_value(frozen_input).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        params.insert(
            "output".into(),
            serde_json::to_value(&self.output).map_err(|e| Error::Protocol(e.to_string()))?,
        );
        let mut result =
            process.invoke(&self.operation, serde_json::Value::Object(params), None)?;
        if !self.output.exists() {
            return Err(Error::Protocol(format!(
                "{} wrote no {}",
                manifest.id,
                self.output.display()
            )));
        }
        if self.operation == "tree" {
            let mut trees = vec![self.output.canonicalize()?];
            for path in &result.outputs {
                let path = PathBuf::from(path);
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nwk"))
                {
                    let path = path.canonicalize()?;
                    if !trees.contains(&path) {
                        trees.push(path);
                    }
                }
            }
            let artifacts = basecraft_ops::links::AlignmentLink::create(
                &self.output,
                &self.source_path,
                frozen_input,
                self.source_version,
                true,
                Some(&self.source_hash),
            )?;
            result
                .outputs
                .extend(artifacts.iter().map(|p| p.to_string_lossy().into_owned()));
            let link = basecraft_ops::links::AlignmentLink::read(&self.output)?;
            for tree in trees.into_iter().skip(1) {
                let metadata = link.share_to(&self.output, &tree)?;
                result.outputs.push(metadata.to_string_lossy().into_owned());
            }
        }
        Ok(result)
    }

    /// Log the run against the document its input came from.
    pub fn record(&self, session: &mut Session, doc: DocId, result: &InvokeResult) -> Result<()> {
        let manifest = Manifest::load(&self.plugin)?;
        let actor = result.tool.as_ref().map_or_else(
            || Actor::Plugin {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
            },
            |t| Actor::Tool {
                tool: t.name.clone(),
                version: t.version.clone(),
            },
        );
        let mut outputs = vec![self.output.canonicalize()?];
        for path in &result.outputs {
            let path = PathBuf::from(path).canonicalize()?;
            if !outputs.contains(&path) {
                outputs.push(path);
            }
        }
        let mut recorded_parameters =
            crate::compute::parameters(&manifest.id, self.params.clone(), self.execution_budget());
        recorded_parameters.insert("job_id".into(), json!(self.run_id));
        if let Some(execution) = &result.execution {
            recorded_parameters.insert(
                "_execution".into(),
                serde_json::to_value(execution).map_err(|e| Error::Protocol(e.to_string()))?,
            );
        }
        if let Some(tool) = &result.tool {
            recorded_parameters.insert(
                "_observed_tool".into(),
                serde_json::to_value(tool).map_err(|e| Error::Protocol(e.to_string()))?,
            );
        }
        session.snapshot_file(&self.retained_input()?)?;
        session.log_tool_run(
            doc,
            actor,
            (&manifest.id, &self.operation),
            serde_json::Value::Object(recorded_parameters),
            std::slice::from_ref(&self.input_key),
            &outputs,
        )?;
        session.publish_analysis_snapshot(
            doc,
            &self.output,
            std::slice::from_ref(&self.input_key),
            &self.source_record,
        )?;
        Ok(())
    }
}

/// `<stem>.<suffix>.<ext>` beside `file`, numbered if that name is taken:
/// `rbcL.fasta` gives `rbcL.mafft.fasta`, then `rbcL.mafft-2.fasta`.
pub fn output_beside(file: &Path, suffix: &str, ext: &str) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // Drop the extension, and an `.aln` before it.
    let mut stem = name.split('.').next().unwrap_or("alignment").to_string();
    if stem.is_empty() {
        stem = "alignment".into();
    }
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(format!("{stem}.{suffix}.{ext}"))
        } else {
            dir.join(format!("{stem}.{suffix}-{n}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_number_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rbcL.aln.fasta");
        let first = output_beside(&file, "mafft", "fasta");
        assert_eq!(first, dir.path().join("rbcL.mafft.fasta"));
        std::fs::write(&first, "").unwrap();
        assert_eq!(
            output_beside(&file, "mafft", "fasta"),
            dir.path().join("rbcL.mafft-2.fasta")
        );
    }
}
