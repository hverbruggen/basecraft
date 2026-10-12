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
        Ok(Self {
            plugin: plugin.to_path_buf(),
            operation: operation.to_string(),
            input: path,
            input_key: (key, hash),
            output,
            params: [("job_id".into(), json!(ulid::Ulid::generate().to_string()))]
                .into_iter()
                .collect(),
            source_path: session.root().join(session.doc_key(doc)),
            source_version: session.alignment(doc).version(),
            source_hash: basecraft_io::hash_file(&session.root().join(session.doc_key(doc)))
                .map_err(basecraft_ops::Error::from)?,
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
        Ok(Self {
            plugin: plugin.to_path_buf(),
            operation: operation.into(),
            input,
            input_key: (session.doc_key(doc).into(), hash),
            output,
            params: [("job_id".into(), json!(ulid::Ulid::generate().to_string()))]
                .into_iter()
                .collect(),
            source_path: source_path.clone(),
            source_version: session.tree(doc).version(),
            source_hash: basecraft_io::hash_file(&source_path)
                .map_err(basecraft_ops::Error::from)?,
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
        Ok(Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash),
            output,
            params: [("job_id".into(), json!(ulid::Ulid::generate().to_string()))]
                .into_iter()
                .collect(),
            source_version: session.generation(doc),
            source_hash: basecraft_io::hash_file(&source_path)
                .map_err(basecraft_ops::Error::from)?,
            source_path,
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
        Ok(Self {
            plugin: plugin.into(),
            operation: operation.into(),
            input: snapshot,
            input_key: (input.display().to_string(), hash),
            output,
            params: [("job_id".into(), json!(ulid::Ulid::generate().to_string()))]
                .into_iter()
                .collect(),
            source_path: self.source_path.clone(),
            source_version: self.source_version,
            source_hash: self.source_hash.clone(),
            _dir: dir,
        })
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
        session.snapshot_file(&self.input)?;
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
            id: ulid::Ulid::generate().to_string(),
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
                    if self
                        .params
                        .get("job_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(&native.id)
                        && native.method == self.operation
                    {
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
        let manifest = Manifest::load(&self.plugin)?;
        let mut process = PluginProcess::spawn(&manifest)?;
        let mut params = self.params.clone();
        params
            .entry("original_source")
            .or_insert(json!(self.source_path));
        params.insert("input".into(), json!(self.input));
        params.insert("output".into(), json!(self.output));
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
                &self.input,
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
        session.log_tool_run(
            doc,
            actor,
            (&manifest.id, &self.operation),
            serde_json::Value::Object(self.params.clone()),
            std::slice::from_ref(&self.input_key),
            &outputs,
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
