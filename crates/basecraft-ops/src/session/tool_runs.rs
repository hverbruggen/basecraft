//! Recording tool runs and job outcomes.
use super::Session;
use super::{Actor, DocId, Op, OpResult, Result};
use super::{PathBuf, invalid};

/// What one tool run records. Every route (GUI, CLI, Python, MCP, plugins,
/// imports) reaches the document's log through `Session::record_tool_run`.
pub(crate) struct ToolRunRecord<'a> {
    pub(crate) activity: (&'a str, &'a str),
    pub(crate) params: serde_json::Value,
    /// Inputs with the hashes of exactly what the tool was given.
    pub(crate) inputs: &'a [(String, String)],
    /// Outputs, hashed here.
    pub(crate) outputs: &'a [PathBuf],
    pub(crate) input_run: Option<basecraft_protocol::records::ActivityInputRun>,
    pub(crate) contributions: Option<basecraft_protocol::records::ActivityContributions>,
    /// A run on a referenced source need not promote it to an edit journal.
    pub(crate) referenced: bool,
}

impl Session {
    /// The one way a tool run reaches the log: one typed `tool.run` entry.
    pub(crate) fn record_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        record: ToolRunRecord<'_>,
    ) -> Result<()> {
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
        let (plugin, operation) = record.activity;
        let entry = self.entry(
            id,
            actor,
            Op::ToolRun {
                plugin: plugin.to_string(),
                operation: operation.to_string(),
                params: record.params,
                inputs: record.inputs.iter().map(|(k, _)| k.clone()).collect(),
                outputs: keys,
                input_run: record.input_run,
                contributions: record.contributions,
            },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// Log a plugin run that read and wrote files, such as an aligner. The run
    /// changes no open document.
    pub fn log_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        activity: (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.record_tool_run(
            id,
            actor,
            ToolRunRecord {
                activity,
                params,
                inputs,
                outputs,
                input_run: None,
                contributions: None,
                referenced: false,
            },
        )
    }

    pub fn log_referenced_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        activity: (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.record_tool_run(
            id,
            actor,
            ToolRunRecord {
                activity,
                params,
                inputs,
                outputs,
                input_run: None,
                contributions: None,
                referenced: true,
            },
        )
    }

    /// Record explicit ownership of proposal/import edits. Generic no-output
    /// activities never acquire ownership merely by following an edit.
    pub fn log_contributing_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        activity: (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        edits: &[String],
    ) -> Result<()> {
        self.record_contributions(id, actor, activity, params, inputs, edits, None)
    }

    /// As `log_contributing_tool_run`, with the run that already retains the
    /// exact input, if the caller made one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_contributions(
        &mut self,
        id: DocId,
        actor: Actor,
        activity: (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        edits: &[String],
        mut input_run: Option<basecraft_protocol::records::ActivityInputRun>,
    ) -> Result<()> {
        for edit in edits {
            if self.tracked
                && !self.events.iter().any(|e| {
                    e.id == *edit
                        && e.doc == self.docs[id.0].key
                        && e.actor == actor
                        && matches!(
                            e.op,
                            Op::FeatureAdd { .. }
                                | Op::FeatureUpdate { .. }
                                | Op::TreeAddSupport { .. }
                                | Op::SeqReplace { .. }
                        )
                })
            {
                return Err(invalid(
                    "activity names an unknown or differently owned edit",
                ));
            }
        }
        // Retain the exact submitted input once, in its own run, unless the
        // caller already did.
        if self.tracked
            && !edits.is_empty()
            && input_run.is_none()
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
            && let Some((source, hash)) = inputs.first()
        {
            let source = self.resolve(source);
            let ledger = crate::records::input_evidence::reference_input_with_dependencies(
                &self.resolve(self.doc_key(id)?),
                &source,
                hash,
                &[],
                "Retained annotation input snapshot",
            )?;
            input_run = Some(input_run_of(&ledger)?);
        }
        self.record_tool_run(
            id,
            actor,
            ToolRunRecord {
                activity,
                params,
                inputs,
                outputs: &[],
                input_run,
                contributions: Some(basecraft_protocol::records::ActivityContributions {
                    schema_version: 1,
                    edits: edits.to_vec(),
                    extra: std::collections::BTreeMap::new(),
                }),
                referenced: false,
            },
        )
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
