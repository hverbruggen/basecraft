//! Exact identity preprocessing, followed by ASAP or public UPGMA and GMYC.
//! Every stage is an ordinary public file-plugin job with retained outcomes.
use crate::{Error, FileJob, Result};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{
    delimitation::{JobRecord, JobStatus},
    plugin::InvokeResult,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

/// The fit stage of a reserved alignment workflow (`basecraft.host` descriptors).
pub fn workflow_fit(workflow: &str) -> Option<&'static str> {
    match workflow {
        "asap_alignment" => Some("asap"),
        "gmyc_alignment_single" => Some("gmyc_single"),
        "gmyc_alignment_multiple" => Some("gmyc_multiple"),
        _ => None,
    }
}

pub struct AlignmentJob {
    pub preparation: FileJob,
    pub unique_haplotypes: bool,
    pub fit_method: String,
    pub output: PathBuf,
}
struct Stage {
    job: FileJob,
    outcome: Result<InvokeResult>,
    elapsed: f64,
}
pub struct PipelineOutcome {
    stages: Vec<Stage>,
    unavailable: Option<String>,
    pub output: PathBuf,
    primary: Option<PathBuf>,
}
impl AlignmentJob {
    /// No session access: this method can run on a worker thread.
    pub fn run(mut self) -> PipelineOutcome {
        let primary = self.preparation.species_workspace_target(&self.output);
        self.output = self.preparation.output.with_file_name("fit.json");
        let mut result = PipelineOutcome {
            stages: vec![],
            unavailable: None,
            output: self.output.clone(),
            primary,
        };
        let started = Instant::now();
        let outcome = self.preparation.run();
        result.stages.push(Stage {
            job: self.preparation,
            outcome,
            elapsed: started.elapsed().as_secs_f64(),
        });
        if result.stages[0].outcome.is_err() {
            return result;
        }
        if self.fit_method == "asap" {
            return run_asap(result, self.unique_haplotypes);
        }
        let prepare = &result.stages[0].job;
        let setup = (|| -> Result<FileJob> {
            if !["gmyc_single", "gmyc_multiple"].contains(&self.fit_method.as_str()) {
                return Err(Error::Protocol(
                    "alignment pipeline requires single or multiple GMYC".into(),
                ));
            }
            let metadata: Value = serde_json::from_slice(&std::fs::read(&prepare.output)?)?;
            let counts = &metadata["metrics"]["haplotypes"];
            let retained = if self.unique_haplotypes {
                counts["unique"].as_array().map_or(0, Vec::len)
            } else {
                counts["original_count"]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0)
            };
            if retained < 2 {
                return Err(Error::Protocol("GMYC unavailable: fewer than two retained haplotypes; no tree or fit was invented".into()));
            }
            let registry = prepare.stage_registry()?;
            let plugin = registry
                .resolve("basecraft.distance/tree@1", None)?
                .package
                .manifest
                .dir
                .clone();
            let mut tree = following_alignment(
                prepare,
                self.unique_haplotypes,
                (&plugin, "tree"),
                PathBuf::from(format!("{}.upgma.nwk", self.output.display())),
            )?;
            tree.params.insert("method".into(), "upgma".into());
            for name in [
                "model",
                "min_overlap",
                "threads",
                "tree_threads",
                "cancel_file",
                "progress",
                "progress_job_id",
            ] {
                if let Some(value) = prepare.params.get(name) {
                    tree.params.insert(name.into(), value.clone());
                }
            }
            Ok(tree)
        })();
        let tree = match setup {
            Ok(tree) => tree,
            Err(error) => {
                result.unavailable = Some(error.to_string());
                return result;
            }
        };
        if cancelled(prepare) {
            result.unavailable = Some("GMYC cancelled before tree inference".into());
            return result;
        }
        let started = Instant::now();
        let outcome = tree.run();
        result.stages.push(Stage {
            job: tree,
            outcome,
            elapsed: started.elapsed().as_secs_f64(),
        });
        if result.stages[1].outcome.is_err() {
            return result;
        }
        let prepare = &result.stages[0].job;
        let setup = (|| -> Result<FileJob> {
            let mut fit = result.stages[1].job.following(
                &result.stages[1].job.output,
                (&prepare.plugin, &self.fit_method),
                self.output,
            )?;
            fit.params.extend(prepare.params.clone());
            fit.params.insert(
                "job_id".into(),
                json!(format!(
                    "{}-fit",
                    prepare.params["job_id"].as_str().unwrap_or("alignment")
                )),
            );
            sampling_parameters(prepare, &mut fit, self.unique_haplotypes)?;
            fit.params.insert("rooted".into(), true.into());
            fit.params.insert(
                "units".into(),
                json!("relative time from equal-rate UPGMA distances; uncalibrated"),
            );
            fit.params.insert(
                "tree_inference".into(),
                "UPGMA equal-rate clustering from retained aligned sequences".into(),
            );
            Ok(fit)
        })();
        let mut fit = match setup {
            Ok(fit) => fit,
            Err(error) => {
                result.unavailable = Some(error.to_string());
                return result;
            }
        };
        fit.publish_species_workspace_at(result.primary.clone());
        result.output.clone_from(&fit.output);
        let started = Instant::now();
        let outcome = fit.run();
        result.stages.push(Stage {
            job: fit,
            outcome,
            elapsed: started.elapsed().as_secs_f64(),
        });
        result
    }
}
fn following_alignment(
    prepare: &FileJob,
    unique: bool,
    operation: (&std::path::Path, &str),
    output: PathBuf,
) -> Result<FileJob> {
    let metadata: Value = serde_json::from_slice(&std::fs::read(&prepare.output)?)?;
    let source = if unique {
        PathBuf::from(format!("{}.unique.fasta", prepare.output.display()))
    } else {
        prepare.input.clone()
    };
    let expected = if unique {
        metadata["job"]["outputs"]
            .get(source.display().to_string())
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Protocol("Preparation has no representative alignment hash".into())
            })?
    } else {
        &prepare.input_key.1
    };
    let job = prepare.following(&source, operation, output)?;
    if job.input_key.1 != expected {
        return Err(Error::Protocol(
            "Prepared alignment changed; repeat preparation".into(),
        ));
    }
    Ok(job)
}

/// Carry the exact retained source and reversible mapping into every fit.
fn sampling_parameters(prepare: &FileJob, fit: &mut FileJob, unique: bool) -> Result<()> {
    let metadata: Value = serde_json::from_slice(&std::fs::read(&prepare.output)?)?;
    let counts = &metadata["metrics"]["haplotypes"];
    let snapshot = prepare.input.display().to_string();
    let evidence = prepare.species_input_evidence()?;
    fit.params.extend([
        ("deduplicate".into(), json!(unique)),
        ("alignment_input_hash".into(), json!(prepare.input_key.1)),
        ("alignment_snapshot".into(), json!(snapshot)),
        ("original_source".into(), evidence["path"].clone()),
        ("original_count".into(), counts["original_count"].clone()),
        (
            "retained_count".into(),
            if unique {
                json!(counts["unique"].as_array().map_or(0, Vec::len))
            } else {
                counts["original_count"].clone()
            },
        ),
        (
            "sequence_sampling".into(),
            json!(if unique {
                "unique aligned DNA haplotypes"
            } else {
                "all specimens"
            }),
        ),
    ]);
    fit.params.insert("alignment_evidence".into(), evidence);
    if unique {
        let mapping = format!("{}.haplotypes.json", prepare.output.display());
        let hash = metadata["job"]["outputs"]
            .get(&mapping)
            .ok_or_else(|| Error::Protocol("Preparation has no mapping hash".into()))?;
        fit.params.insert("mapping".into(), json!(mapping));
        fit.params.insert("mapping_hash".into(), hash.clone());
    }
    Ok(())
}

fn run_asap(mut result: PipelineOutcome, unique: bool) -> PipelineOutcome {
    let prepare = &result.stages[0].job;
    let setup = (|| -> Result<FileJob> {
        if cancelled(prepare) {
            return Err(Error::Protocol("ASAP cancelled before fitting".into()));
        }
        let mut fit = following_alignment(
            prepare,
            unique,
            (&prepare.plugin, "asap"),
            result.output.clone(),
        )?;
        fit.params.extend(prepare.params.clone());
        sampling_parameters(prepare, &mut fit, unique)?;
        Ok(fit)
    })();
    match setup {
        Ok(mut job) => {
            job.publish_species_workspace_at(result.primary.clone());
            result.output.clone_from(&job.output);
            let started = Instant::now();
            let outcome = job.run();
            result.stages.push(Stage {
                job,
                outcome,
                elapsed: started.elapsed().as_secs_f64(),
            });
        }
        Err(error) => result.unavailable = Some(error.to_string()),
    }
    result
}

fn cancelled(job: &FileJob) -> bool {
    job.params
        .get("cancel_file")
        .and_then(Value::as_str)
        .is_some_and(|path| std::path::Path::new(path).exists())
}
impl PipelineOutcome {
    /// Retain every completed/failed/cancelled stage, then return the final result.
    pub fn record(self, session: &mut Session, doc: DocId) -> Result<InvokeResult> {
        let inputs = self
            .stages
            .first()
            .map_or_else(BTreeMap::new, |stage| [stage.job.input_key.clone()].into());
        let mut last = None;
        let mut outputs = Vec::new();
        for stage in self.stages {
            stage
                .job
                .record_terminal(session, doc, &stage.outcome, stage.elapsed)
                .map_err(|error| {
                    Error::Protocol(format!(
                        "Recording {} outcome at {}: {error}",
                        stage.job.operation,
                        stage.job.output.display()
                    ))
                })?;
            let result = stage.outcome?;
            stage.job.record(session, doc, &result).map_err(|error| {
                Error::Protocol(format!(
                    "Recording {} at {}: {error}",
                    stage.job.operation,
                    stage.job.output.display()
                ))
            })?;
            outputs.extend(result.outputs.iter().cloned());
            last = Some(result);
        }
        if let Some(reason) = self.unavailable {
            let report = JobRecord {
                id: ulid::Ulid::generate().to_string(),
                generation: session.generation(doc),
                status: if reason.contains("cancelled") {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Unavailable
                },
                method: "delimitation_alignment".into(),
                inputs,
                outputs: BTreeMap::new(),
                diagnostics: vec![reason.clone()],
                elapsed_seconds: 0.0,
            };
            let path = PathBuf::from(format!("{}.run.json", self.output.display()));
            basecraft_io::write_atomic(&path, |writer| {
                serde_json::to_writer_pretty(
                    writer,
                    &json!({"version":1,"job":report,"workspace":null,"metrics":{}}),
                )?;
                Ok(())
            })
            .map_err(basecraft_ops::Error::from)?;
            let mut record = report;
            record
                .outputs
                .insert(path.display().to_string(), session.snapshot_file(&path)?);
            session.log_job_finished(doc, record)?;
            return Err(Error::Protocol(reason));
        }
        let mut result =
            last.ok_or_else(|| Error::Protocol("pipeline produced no stages".into()))?;
        outputs.sort();
        outputs.dedup();
        if let Some(primary) = self.primary {
            if let Some(index) = outputs
                .iter()
                .position(|p| std::path::Path::new(p) == primary)
            {
                outputs.swap(0, index);
            }
        }
        result.outputs = outputs;
        Ok(result)
    }
}

/// A worker owns inputs until its outcome is recorded on the session thread.
pub enum FileWorkflow {
    Single(FileJob),
    Alignment(AlignmentJob),
}
pub enum WorkflowOutcome {
    Single {
        job: Box<FileJob>,
        outcome: Result<InvokeResult>,
        elapsed: f64,
    },
    Alignment(PipelineOutcome),
}
impl FileWorkflow {
    pub fn run(self) -> WorkflowOutcome {
        match self {
            Self::Single(job) => {
                let started = Instant::now();
                let outcome = job.run();
                WorkflowOutcome::Single {
                    job: Box::new(job),
                    outcome,
                    elapsed: started.elapsed().as_secs_f64(),
                }
            }
            Self::Alignment(job) => WorkflowOutcome::Alignment(job.run()),
        }
    }
}
impl WorkflowOutcome {
    pub fn output(&self) -> &std::path::Path {
        match self {
            Self::Single { job, .. } => &job.output,
            Self::Alignment(outcome) => &outcome.output,
        }
    }
    pub fn record(self, session: &mut Session, doc: DocId) -> Result<InvokeResult> {
        match self {
            Self::Single {
                job,
                outcome,
                elapsed,
            } => {
                job.record_terminal(session, doc, &outcome, elapsed)?;
                let result = outcome?;
                job.record(session, doc, &result)?;
                Ok(result)
            }
            Self::Alignment(outcome) => outcome.record(session, doc),
        }
    }
}
