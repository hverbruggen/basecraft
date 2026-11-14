//! Recording tool runs and job outcomes.
use super::Session;
use super::{Actor, DocId, Op, OpResult, Result};
use super::{PathBuf, invalid};
use std::path::Path;

/// What one tool run records. Every route (GUI, CLI, Python, MCP, plugins,
/// imports) reaches the document's log through `Session::record_tool_run`.
pub struct ToolRunRecord<'a> {
    pub activity: (&'a str, &'a str),
    pub params: serde_json::Value,
    /// Inputs with the hashes of exactly what the tool was given.
    pub inputs: &'a [(String, String)],
    /// Outputs with their hashes.
    pub outputs: &'a [(PathBuf, String)],
    /// The run that already retains the exact input, if the caller made one.
    pub input_run: Option<basecraft_protocol::records::ActivityInputRun>,
    /// The edits this run made in the document, for proposals and imports.
    /// Runs that edit nothing never take ownership of later edits.
    pub edits: Option<&'a [String]>,
    /// A run on a referenced source need not promote it to an edit journal.
    pub referenced: bool,
    /// The run folder, when the run has one. Its `execution.json` is what
    /// history and methods read about how the tool ran.
    pub run: Option<&'a crate::records::ArtifactRun>,
}

impl<'a> ToolRunRecord<'a> {
    /// A run that read `inputs` and wrote `outputs`, such as an aligner.
    pub fn new(
        activity: (&'a str, &'a str),
        params: serde_json::Value,
        inputs: &'a [(String, String)],
        outputs: &'a [(PathBuf, String)],
    ) -> Self {
        Self {
            activity,
            params,
            inputs,
            outputs,
            input_run: None,
            edits: None,
            referenced: false,
            run: None,
        }
    }

    /// `paths` with their hashes, for outputs whose writer did not hash them.
    pub fn hashed(paths: Vec<PathBuf>) -> Result<Vec<(PathBuf, String)>> {
        let mut hashed = Vec::new();
        for path in paths {
            let hash = basecraft_io::hash_file(&path)?;
            hashed.push((path, hash));
        }
        Ok(hashed)
    }

    /// The `tool.run` operation. Its run link is relative to `base`: the
    /// project root, or the document's own folder for a loose file. A run
    /// folder that cannot be named relative to it is refused.
    pub(crate) fn op(
        self,
        base: &Path,
        outputs: Vec<String>,
        input_run: Option<basecraft_protocol::records::ActivityInputRun>,
        contributions: Option<basecraft_protocol::records::ActivityContributions>,
    ) -> Result<Op> {
        let (plugin, operation) = self.activity;
        let run = match self.run {
            Some(run) => Some(basecraft_protocol::ops::RunLink {
                id: run.id().into(),
                folder: run_folder(base, run.directory())?,
            }),
            None => None,
        };
        Ok(Op::ToolRun {
            plugin: plugin.to_string(),
            operation: operation.to_string(),
            params: self.params,
            inputs: self.inputs.iter().map(|(k, _)| k.clone()).collect(),
            outputs,
            input_run,
            contributions,
            run,
        })
    }
}

/// A run folder named relative to `base`, as a run link holds it.
fn run_folder(base: &Path, folder: &Path) -> Result<String> {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    crate::records::relative(&canonical(base), &canonical(folder))
        .ok()
        .filter(|relative| !Path::new(relative).is_absolute())
        .ok_or_else(|| {
            invalid(format!(
                "the run folder {} cannot be named relative to {}",
                folder.display(),
                base.display()
            ))
        })
}

impl Session {
    /// The folder a document's run links are relative to: the project root,
    /// or the file's own folder for a loose file.
    pub(crate) fn run_base(&self, id: DocId) -> Result<PathBuf> {
        let path = self.resolve(self.doc_key(id)?);
        crate::project::Project::base_of(self.project.as_ref(), &path)
    }

    /// Replace a saved document's file with `bytes` a tool made from it, such
    /// as numbered record names (D29): record the run, write the file and
    /// publish the history, which then describes the new file. The open
    /// document keeps the old content, so the caller opens the file again.
    pub fn publish_rewrite(
        &mut self,
        id: DocId,
        activity: (&str, &str),
        params: serde_json::Value,
        bytes: &[u8],
    ) -> Result<()> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or(crate::Error::UnknownDocument { id })?;
        if !slot.is_clean() {
            return Err(crate::Error::NotReady(
                "Save or undo the changes first".into(),
            ));
        }
        let key = slot.key.clone();
        let target = self.resolve(&key);
        self.check_save_source(id, None)?;
        let before = basecraft_io::hash_file(&target)?;
        let after = basecraft_io::hash_bytes(bytes);
        let actor = self.actor.clone();
        // Recorded before the file changes: the run logs the file as opened.
        self.record_tool_run(
            id,
            actor,
            ToolRunRecord::new(
                activity,
                params,
                &[(key.clone(), before)],
                &[(target.clone(), after.clone())],
            ),
        )?;
        {
            let _target = crate::records::save_guard::lock(&target)?;
            basecraft_io::write_atomic(&target, |w| std::io::Write::write_all(w, bytes))?;
        }
        for (accepted, hash) in &mut self.docs[id.0].accepted_hashes {
            if *accepted == key {
                hash.clone_from(&after);
            }
        }
        self.refresh_source_stamps(id);
        self.save_history(id)
    }

    /// The one way a tool run reaches the log: one typed `tool.run` entry.
    pub fn record_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        mut record: ToolRunRecord<'_>,
    ) -> Result<()> {
        let mut input_run = record.input_run.take();
        let contributions = match record.edits {
            Some(edits) => {
                self.check_owned_edits(id, &actor, edits)?;
                if input_run.is_none() {
                    input_run = self.retain_input(id, record.inputs, edits)?;
                }
                Some(basecraft_protocol::records::ActivityContributions {
                    schema_version: 1,
                    edits: edits.to_vec(),
                    extra: std::collections::BTreeMap::new(),
                })
            }
            None => None,
        };
        if record.referenced {
            self.reference_baseline(id)?;
        } else {
            self.track(id)?;
        }
        let mut hashes = record.inputs.to_vec();
        let mut keys = Vec::new();
        for (path, hash) in record.outputs {
            let key = self.relative(path);
            hashes.push((key.clone(), hash.clone()));
            keys.push(key);
        }
        let op = record.op(&self.run_base(id)?, keys, input_run, contributions)?;
        let entry = self.entry(
            id,
            actor,
            op,
            OpResult {
                notes: Vec::new(),
                hashes,
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// A run may claim only edits this actor made in this document.
    fn check_owned_edits(&self, id: DocId, actor: &Actor, edits: &[String]) -> Result<()> {
        if self.tracked {
            let slot = &self.docs[id.0];
            let owned: Vec<_> = slot
                .recorded
                .iter()
                .chain(&slot.unsaved)
                .filter(|e| {
                    edits.contains(&e.id)
                        && e.actor == *actor
                        && matches!(
                            e.op,
                            Op::FeatureAdd { .. }
                                | Op::FeatureUpdate { .. }
                                | Op::TreeAddSupport { .. }
                                | Op::SeqReplace { .. }
                        )
                })
                .collect();
            if !edits.iter().all(|edit| owned.iter().any(|e| e.id == *edit)) {
                return Err(invalid(
                    "activity names an unknown or differently owned edit",
                ));
            }
        }
        Ok(())
    }

    /// Retain the exact submitted input of an editing run once, in its own run.
    fn retain_input(
        &self,
        id: DocId,
        inputs: &[(String, String)],
        edits: &[String],
    ) -> Result<Option<basecraft_protocol::records::ActivityInputRun>> {
        let Some((source, hash)) = inputs.first() else {
            return Ok(None);
        };
        if !self.tracked || edits.is_empty() {
            return Ok(None);
        }
        let ledger = crate::records::run_inputs::reference_input_with_dependencies(
            &self.resolve(self.doc_key(id)?),
            &self.resolve(source),
            hash,
            &[],
            "Retained annotation input snapshot",
            self.project.as_ref(),
        )?;
        Ok(Some(input_run_of(&ledger)?))
    }

    /// Retain failure and cancellation diagnostics without creating a hypothesis.
    pub fn log_job_finished(
        &mut self,
        id: DocId,
        report: basecraft_protocol::records::JobRecord,
    ) -> Result<()> {
        self.track(id)?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::JobFinished { report },
            OpResult::default(),
        );
        self.write(&entry)
    }

    /// A referenced job outcome does not require promoting the source to an edit journal.
    pub fn log_referenced_job_finished(
        &mut self,
        id: DocId,
        report: basecraft_protocol::records::JobRecord,
    ) -> Result<()> {
        self.reference_baseline(id)?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::JobFinished { report },
            OpResult::default(),
        );
        self.write(&entry)
    }
}

/// The typed reference to a run that retains an activity's exact input.
pub(crate) fn input_run_of(
    ledger: &crate::records::ArtifactRun,
) -> Result<basecraft_protocol::records::ActivityInputRun> {
    let record = ledger
        .directory()
        .join("records")
        .join(format!("{}.json", ledger.id()));
    Ok(basecraft_protocol::records::ActivityInputRun {
        schema_version: 1,
        id: ledger.id().into(),
        hash: basecraft_io::hash_file(&record)?,
        extra: std::collections::BTreeMap::new(),
    })
}
