//! Exact identity preprocessing, public UPGMA inference, then reference GMYC.
//! Every stage is an ordinary public file-plugin job with retained outcomes.
use crate::{Error, FileJob, Result};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::{
    delimitation::{JobRecord, JobStatus},
    plugin::InvokeResult,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

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
}
impl AlignmentJob {
    /// No session access: this method can run on a worker thread.
    pub fn run(self) -> PipelineOutcome {
        let mut result = PipelineOutcome {
            stages: vec![],
            unavailable: None,
            output: self.output.clone(),
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
            let source = if self.unique_haplotypes {
                PathBuf::from(format!("{}.unique.fasta", prepare.output.display()))
            } else {
                prepare.input.clone()
            };
            let plugin = prepare
                .plugin
                .parent()
                .ok_or_else(|| Error::Protocol("plugin directory has no parent".into()))?
                .join("distance");
            let mut tree = prepare.following(
                &source,
                (&plugin, "tree"),
                PathBuf::from(format!("{}.upgma.nwk", self.output.display())),
            )?;
            tree.params.insert("method".into(), "upgma".into());
            for name in ["model", "min_overlap", "threads", "tree_threads"] {
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
            let mut fit = prepare.following(
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
            let metadata: Value = serde_json::from_slice(&std::fs::read(&prepare.output)?)?;
            let counts = &metadata["metrics"]["haplotypes"];
            fit.params.insert(
                "alignment_input_hash".into(),
                prepare.input_key.1.clone().into(),
            );
            fit.params
                .insert("original_count".into(), counts["original_count"].clone());
            fit.params.insert(
                "retained_count".into(),
                if self.unique_haplotypes {
                    json!(counts["unique"].as_array().map_or(0, Vec::len))
                } else {
                    counts["original_count"].clone()
                },
            );
            fit.params.insert("rooted".into(), true.into());
            fit.params.insert(
                "tree_inference".into(),
                "UPGMA equal-rate clustering from retained aligned sequences".into(),
            );
            fit.params.insert(
                "sequence_sampling".into(),
                if self.unique_haplotypes {
                    "unique aligned DNA haplotypes"
                } else {
                    "all specimens"
                }
                .into(),
            );
            if self.unique_haplotypes {
                fit.params.insert(
                    "mapping".into(),
                    json!(format!("{}.haplotypes.json", prepare.output.display())),
                );
            }
            Ok(fit)
        })();
        let fit = match setup {
            Ok(fit) => fit,
            Err(error) => {
                result.unavailable = Some(error.to_string());
                return result;
            }
        };
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
                .record_terminal(session, doc, &stage.outcome, stage.elapsed)?;
            let result = stage.outcome?;
            stage.job.record(session, doc, &result)?;
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
                method: "gmyc_alignment".into(),
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
