//! Recording tool runs and job outcomes.
use super::{Actor, DocId, Op, OpResult, Result};

impl Session {
    /// Log a plugin run that read and wrote files, such as an aligner. Its
    /// `inputs` come with the hashes of exactly what the tool was given;
    /// the outputs are hashed here. The run changes no open document.
    pub fn log_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        (plugin, operation): (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.track(id)?;
        self.flush()?;
        let mut hashes = inputs.to_vec();
        let mut keys = Vec::new();
        for path in outputs {
            let key = self.relative(path);
            hashes.push((key.clone(), basecraft_io::hash_file(path)?));
            keys.push(key);
        }
        let entry = self.entry(
            id,
            actor,
            Op::ToolRun {
                plugin: plugin.to_string(),
                operation: operation.to_string(),
                params,
                inputs: inputs.iter().map(|(k, _)| k.clone()).collect(),
                outputs: keys,
            },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// Record explicit ownership of proposal/import edits. Generic no-output
    /// activities never acquire ownership merely by following an edit.
    pub fn log_contributing_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        activity: (&str, &str),
        mut params: serde_json::Value,
        inputs: &[(String, String)],
        edits: &[String],
    ) -> Result<()> {
        if !params.is_object() {
            return Err(invalid(
                "contributing activity parameters must be an object",
            ));
        }
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
        // The exact submitted snapshot already lives in the registered run.
        // Reuse the support-import reference owner for its source edge; do not
        // duplicate execution receipts or copy the input again. Legacy callers
        // without exact submitted bytes keep their evidence explicitly unknown.
        if self.tracked
            && !edits.is_empty()
            && params.get("input_run").is_none()
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
            && let Some((source, hash)) = inputs.first()
        {
            let source = self.resolve(source);
            let ledger = crate::input_evidence::reference_input_with_dependencies(
                &self.resolve(self.doc_key(id)),
                &source,
                hash,
                &[],
                "Retained annotation input snapshot",
            )?;
            let record = ledger
                .directory()
                .join("records")
                .join(format!("{}.json", ledger.id()));
            params["input_run"] = serde_json::json!({"schema_version":1,"id":ledger.id(),"hash":basecraft_io::hash_file(&record)?});
        }
        params["contributions"] = serde_json::json!({"schema_version":1,"edits":edits});
        self.log_tool_run(id, actor, activity, params, inputs, &[])
    }

    pub fn log_referenced_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        (plugin, operation): (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.reference_baseline(id)?;
        self.flush()?;
        let mut hashes = inputs.to_vec();
        let mut keys = Vec::new();
        for path in outputs {
            let key = self.relative(path);
            hashes.push((key.clone(), basecraft_io::hash_file(path)?));
            keys.push(key);
        }
        let entry = self.entry(
            id,
            actor,
            Op::ToolRun {
                plugin: plugin.to_string(),
                operation: operation.to_string(),
                params,
                inputs: inputs.iter().map(|(k, _)| k.clone()).collect(),
                outputs: keys,
            },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// Retain failure and cancellation diagnostics without creating a hypothesis.
    pub fn log_job_finished(
        &mut self,
        id: DocId,
        report: basecraft_protocol::delimitation::JobRecord,
    ) -> Result<()> {
        self.track(id)?;
        self.flush()?;
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
        report: basecraft_protocol::delimitation::JobRecord,
    ) -> Result<()> {
        self.reference_baseline(id)?;
        self.flush()?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::JobFinished { report },
            OpResult::default(),
        );
        self.write(&entry)
    }
}
