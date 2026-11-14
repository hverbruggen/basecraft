//! Registered sub-runs for explicit alignment workflows. The parent alone owns
//! the receiving document and its revision/application/undo boundary.
use crate::error::protocol;
use crate::{Error, Result, jobs::JobManager, policy::SessionPolicy, registry::Registry};
use basecraft_protocol::{
    operation::{InputReference, OperationOutcome, OperationRequest, RuntimeState},
    plugin::InvokeResult,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) fn workflow_fit(workflow: &str) -> Option<&'static str> {
    match workflow {
        "asap_alignment" => Some("asap"),
        "gmyc_alignment_single" => Some("gmyc_single"),
        "gmyc_alignment_multiple" => Some("gmyc_multiple"),
        _ => None,
    }
}

pub(crate) struct AlignmentWorkflow<'a> {
    pub registry: &'a Registry,
    pub parent: &'a crate::runner::PreparedOperation,
    pub cancelled: &'a AtomicBool,
    pub stages: Vec<OperationOutcome>,
}
impl AlignmentWorkflow<'_> {
    fn stage(&mut self, operation: &str, inputs: &Value, parameters: &Value) -> Result<PathBuf> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(protocol("cancelled before workflow stage"));
        }
        let descriptor = &self.registry.resolve(operation, None)?.descriptor;
        // The workflow descriptor is the superset. Pass only parameters declared
        // by each stage, retaining the normalized nested settings in its receipt.
        let properties = descriptor.parameters["properties"]
            .as_object()
            .ok_or_else(|| protocol("stage parameters must be an object"))?;
        let parameters: serde_json::Map<_, _> = parameters
            .as_object()
            .ok_or_else(|| protocol("workflow parameters must be an object"))?
            .iter()
            .filter(|(key, _)| properties.contains_key(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let request: OperationRequest = serde_json::from_value(json!({
            "operation":operation,"inputs":inputs,"parameters":parameters
        }))?;
        let root = self.parent.run.artifact("stages")?;
        let mut manager = JobManager::default().with_workflow_parent(self.parent);
        let accepted = manager.submit(
            self.registry,
            request,
            SessionPolicy::new(&root, true)?,
            &self.parent.initiator,
        )?;
        self.stages.push(accepted.clone());
        self.persist()?;
        // Stages belong to the parent's project, so their records do too.
        let mut session =
            basecraft_ops::Session::detached(&root).in_project(self.parent.project.clone());
        let outcome = manager.wait_in_session(&accepted.run_id, &mut session, &|| {
            self.cancelled.load(Ordering::Acquire)
        })?;
        let directory = outcome
            .evidence
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| protocol("stage has no retained evidence"))?;
        *self
            .stages
            .last_mut()
            .ok_or_else(|| protocol("stage receipt is missing"))? = outcome.clone();
        self.persist()?;
        if outcome.state != RuntimeState::Completed {
            return Err(protocol(format!(
                "workflow stage {} ended {:?}: {} (evidence: {})",
                operation,
                outcome.state,
                outcome
                    .errors
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
                directory.display()
            )));
        }
        Ok(directory.join("artifacts"))
    }

    fn persist(&self) -> Result<()> {
        let _locks = basecraft_ops::records::lock_in(self.parent.project.as_ref())?;
        basecraft_ops::records::write_plain_json(
            &self.parent.run.artifact("stages.json")?,
            &self.stages,
        )?;
        // Connect immutable completed stage publications to the existing parent
        // ledger. They carry their own execution identity, history and inputs.
        let mut input = basecraft_ops::records::read_input(self.parent.run.directory())?;
        let upstream = &mut input.upstream;
        for stage in &self.stages {
            if stage.state != RuntimeState::Completed {
                continue;
            }
            let directory = Path::new(
                stage
                    .evidence
                    .as_ref()
                    .ok_or_else(|| protocol("stage evidence missing"))?,
            );
            if upstream.iter().any(|parent| parent.id == stage.run_id) {
                continue;
            }
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: (basecraft_ops::records::relative(self.parent.run.directory(), directory)?)
                    .into(),
                id: stage.run_id.clone(),
                hash: basecraft_ops::records::latest_record(directory)?.1,
            });
        }
        basecraft_ops::records::write_input(self.parent.run.directory(), &input)?;
        Ok(())
    }

    fn stage_operation(&self, name: &str) -> Result<String> {
        self.parent
            .operation
            .descriptor
            .needs
            .workflow_stages
            .get(name)
            .cloned()
            .ok_or_else(|| protocol(format!("workflow has no {name} operation")))
    }

    pub fn run(&mut self, method: &str, effective: Value) -> Result<InvokeResult> {
        let fit = workflow_fit(method).ok_or_else(|| protocol("unknown host workflow"))?;
        let root = self.parent.run.artifact("stages")?;
        std::fs::create_dir(&root)?;
        // This durable accepted input is shared by all stages. A child must not
        // refer to the parent's disposable scratch input after it completes.
        let source = root.join("accepted.fasta");
        crate::file_tool::freeze_file(
            &self
                .parent
                .run_path(&self.parent.context.inputs["alignment"][0]),
            &source,
        )?;
        let prepare = self.stage(
            &self.stage_operation("prepare")?,
            &file_inputs(&[("alignment", &source)]),
            &effective,
        )?;
        let report: Value = serde_json::from_slice(&std::fs::read(prepare.join("result.json"))?)?;
        let counts = &report["metrics"]["haplotypes"];
        let unique = effective["unique_haplotypes"]
            .as_bool()
            .unwrap_or(fit != "asap");
        let retained = if unique {
            counts["unique"].as_array().map_or(0, Vec::len)
        } else {
            counts["original_count"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0)
        };
        if fit != "asap" && retained < 2 {
            let diagnostic =
                "GMYC unavailable: fewer than two retained haplotypes; no tree or fit was invented";
            basecraft_ops::records::write_json(
                &self
                    .parent
                    .run_path(&self.parent.context.outputs["artifacts"])
                    .join("fit.json.run.json"),
                &serde_json::to_value(basecraft_protocol::records::WorkflowFailure {
                    version: 1,
                    job: basecraft_protocol::records::WorkflowFailureJob {
                        status: basecraft_protocol::records::JobStatus::Unavailable,
                        diagnostics: vec![diagnostic.into()],
                    },
                    workspace: None,
                    metrics: std::collections::BTreeMap::new(),
                })?,
            )?;
            return Err(Error::Protocol(diagnostic.into()));
        }
        let alignment = if unique {
            prepare.join("result.json.unique.fasta")
        } else {
            source.clone()
        };
        let alignment_hash = basecraft_io::hash_file(&alignment)?;
        if unique
            && !report["job"]["outputs"].as_object().is_some_and(|outputs| {
                outputs.iter().any(|(path, hash)| {
                    Path::new(path).file_name() == alignment.file_name() && hash == &alignment_hash
                })
            })
        {
            return Err(protocol("Prepared representative alignment changed"));
        }
        let mut params = effective.clone();
        params["deduplicate"] = json!(unique);
        params["original_count"] = counts["original_count"].clone();
        params["retained_count"] = json!(retained);
        params["sequence_sampling"] = json!(if unique {
            "unique aligned DNA haplotypes"
        } else {
            "all specimens"
        });
        let fitted = if fit == "asap" { &alignment } else { &source };
        let mut inputs = vec![("alignment", fitted.clone())];
        if fit == "asap" && unique {
            inputs.push(("full_alignment", source.clone()));
        }
        if unique {
            let mapping = prepare.join("result.json.haplotypes.json");
            let hash = basecraft_io::hash_file(&mapping)?;
            if !report["job"]["outputs"].as_object().is_some_and(|outputs| {
                outputs.iter().any(|(path, digest)| {
                    Path::new(path).file_name() == mapping.file_name() && digest == &hash
                })
            }) {
                return Err(protocol("Prepared haplotype mapping changed"));
            }
            params["mapping_hash"] = json!(hash);
            inputs.push(("mapping", mapping));
        }
        if fit != "asap" {
            let mut tree_params = effective;
            tree_params["method"] = json!("upgma");
            let tree = self.stage(
                &self.stage_operation("tree")?,
                &file_inputs(&[("alignment", &alignment)]),
                &tree_params,
            )?;
            inputs.push(("tree", tree.join("tree.nwk")));
            params["rooted"] = json!(true);
            params["units"] = json!("relative time from equal-rate UPGMA distances; uncalibrated");
            params["tree_inference"] =
                json!("UPGMA equal-rate clustering from retained aligned sequences");
        }
        let inputs: Vec<_> = inputs
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        let result = self.stage(
            &self.stage_operation("fit")?,
            &file_inputs(&inputs),
            &params,
        )?;
        let target = &self
            .parent
            .run_path(&self.parent.context.outputs["artifacts"]);
        // Keep nested stage results in their owned runs. The parent's stable fit
        // report is the one logical result consumed by every interface.
        std::fs::copy(result.join("result.json"), target.join("fit.json"))?;
        let mut metadata: Value = serde_json::from_slice(&std::fs::read(target.join("fit.json"))?)?;
        // Report the full accepted alignment for representative-only ASAP too.
        // The fit's mapping and stage receipt retain the representative input.
        if let Some(workspace) = metadata.get_mut("workspace").and_then(Value::as_object_mut) {
            // The workspace names files by absolute path, as every result does.
            let mut evidence = self.parent.context.input_evidence["alignment"][0].clone();
            let absolute = |path: &str| self.parent.run_path(path).display().to_string();
            evidence.original_path = evidence.original_path.as_deref().map(absolute);
            evidence.run_directory = absolute(&evidence.run_directory);
            let evidence = &evidence;
            let stable = evidence.original_path.clone().unwrap_or_else(|| {
                self.parent
                    .run
                    .directory()
                    .join("submitted.fasta")
                    .display()
                    .to_string()
            });
            let fields = workspace
                .entry("exchange_fields")
                .or_insert_with(|| json!({}));
            fields["basecraft_alignment_snapshot"] = json!(stable);
            fields["basecraft_alignment_hash"] = json!(
                evidence
                    .source_hash
                    .as_ref()
                    .unwrap_or(&evidence.submitted_hash)
            );
            fields["basecraft_alignment_evidence_run"] = json!(evidence.run_directory);
            if let Some(hypotheses) = workspace
                .get_mut("hypotheses")
                .and_then(Value::as_array_mut)
            {
                for hypothesis in hypotheses {
                    let params = &mut hypothesis["provenance"]["parameters"];
                    let old = params["alignment_snapshot"].as_str().map(str::to_owned);
                    params["alignment_snapshot"] = json!(stable);
                    params["alignment_input_hash"] = json!(
                        evidence
                            .source_hash
                            .as_ref()
                            .unwrap_or(&evidence.submitted_hash)
                    );
                    let mut linked = serde_json::to_value(evidence)?;
                    linked["path"] = json!(stable);
                    params["alignment_evidence"] = linked;
                    if let Some(old) = old
                        && let Some(inputs) = hypothesis["provenance"]["inputs"].as_object_mut()
                    {
                        inputs.remove(&old);
                        inputs.insert(
                            stable.clone(),
                            json!(
                                evidence
                                    .source_hash
                                    .as_ref()
                                    .unwrap_or(&evidence.submitted_hash)
                            ),
                        );
                    }
                }
            }
        }
        basecraft_ops::records::write_plain_json(&target.join("fit.json"), &metadata)?;
        Ok(InvokeResult {
            artifacts: vec![basecraft_protocol::operation::PluginArtifact {
                role: "artifacts".into(),
                path: "fit.json".into(),
            }],
            message: Some(format!("Completed {method}")),
            ..InvokeResult::default()
        })
    }
}

/// Stage inputs that name one file each.
fn file_inputs(files: &[(&str, &Path)]) -> Value {
    let inputs = files.iter().map(|(name, path)| {
        let path = path.to_string_lossy().into_owned();
        let input = serde_json::to_value(InputReference::File { path }).unwrap_or_default();
        ((*name).to_owned(), Value::Array(vec![input]))
    });
    Value::Object(inputs.collect())
}
