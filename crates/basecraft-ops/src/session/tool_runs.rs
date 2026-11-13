//! Recording tool runs and job outcomes.
use super::Session;
use super::{Actor, DocId, Op, OpResult, Result};
use super::{PathBuf, invalid};

/// What one tool run records. Every route (GUI, CLI, Python, MCP, plugins,
/// imports) reaches the document's log through `Session::record_tool_run`.
pub struct ToolRunRecord<'a> {
    pub activity: (&'a str, &'a str),
    pub params: serde_json::Value,
    /// Inputs with the hashes of exactly what the tool was given.
    pub inputs: &'a [(String, String)],
    /// Outputs, hashed here.
    pub outputs: &'a [PathBuf],
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
        outputs: &'a [PathBuf],
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

    /// The `tool.run` operation, with `outputs` named relative to `root`, the
    /// folder of the log that will hold it.
    pub(crate) fn op(
        self,
        root: &std::path::Path,
        outputs: Vec<String>,
        input_run: Option<basecraft_protocol::records::ActivityInputRun>,
        contributions: Option<basecraft_protocol::records::ActivityContributions>,
    ) -> Op {
        let (plugin, operation) = self.activity;
        Op::ToolRun {
            plugin: plugin.to_string(),
            operation: operation.to_string(),
            params: self.params,
            inputs: self.inputs.iter().map(|(k, _)| k.clone()).collect(),
            outputs,
            input_run,
            contributions,
            run: self.run.map(|run| basecraft_protocol::ops::RunLink {
                id: run.id().into(),
                folder: super::relative(root, run.directory()),
            }),
        }
    }
}

/// Who ran a tool: the external program the run record names, or else the
/// plugin.
pub fn run_actor(run: &crate::records::ArtifactRun, plugin: Actor) -> Actor {
    std::fs::read(run.directory().join("execution.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<basecraft_protocol::records::RunRecord>(&bytes).ok()
        })
        .and_then(|record| record.tool)
        .map_or(plugin, |tool| Actor::Tool {
            tool: tool.name,
            version: tool.version,
        })
}

impl Session {
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
        for path in record.outputs {
            let key = self.relative(path);
            hashes.push((key.clone(), basecraft_io::hash_file(path)?));
            keys.push(key);
        }
        let op = record.op(&self.root, keys, input_run, contributions);
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
            let key = &self.docs[id.0].key;
            let owned = self.events.matching(|e| {
                edits.contains(&e.id)
                    && e.doc == *key
                    && e.actor == *actor
                    && matches!(
                        e.op,
                        Op::FeatureAdd { .. }
                            | Op::FeatureUpdate { .. }
                            | Op::TreeAddSupport { .. }
                            | Op::SeqReplace { .. }
                    )
            })?;
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
        let ledger = crate::records::input_evidence::reference_input_with_dependencies(
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
