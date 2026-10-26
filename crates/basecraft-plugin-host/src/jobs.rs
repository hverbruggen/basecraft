//! Process/session-owned jobs. Accepted runs never depend on a wait request's
//! lifetime; explicit cancellation and manager drop own worker teardown.
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

use crate::{Error, Result, policy::SessionPolicy, registry::Registry, runner::PreparedOperation};
use basecraft_protocol::operation::*;
use serde_json::json;

struct Job {
    prepared: PreparedOperation,
    outcome: Mutex<OperationOutcome>,
    cancelled: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
pub struct JobManager {
    jobs: BTreeMap<String, Arc<Job>>,
    requests: BTreeMap<String, (String, String)>,
}

impl JobManager {
    pub fn submit(
        &mut self,
        registry: &Registry,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: &str,
    ) -> Result<OperationOutcome> {
        let digest = basecraft_io::hash_bytes(&serde_json::to_vec(&request)?);
        if let Some(key) = &request.request_key {
            if let Some((existing, id)) = self.requests.get(key) {
                if existing != &digest {
                    return Err(Error::Protocol(
                        "invalid_arguments: request key reused with different inputs".into(),
                    ));
                }
                return self.get(id);
            }
        }
        if self.jobs.len() >= 128 {
            return Err(Error::Protocol("session job limit reached".into()));
        }
        let operation = registry
            .resolve(&request.operation, request.pins.as_ref())?
            .clone();
        let pins = operation.pins(&registry.generation);
        let key = request.request_key.clone();
        let prepared = PreparedOperation::prepare(operation, request, policy, initiator.into())?;
        let outcome = OperationOutcome {
            schema: 1,
            operation: prepared.operation.id.clone(),
            run_id: prepared.run.id().into(),
            state: RuntimeState::Queued,
            pins,
            requested_parameters: prepared.request.parameters.clone(),
            effective_parameters: prepared.parameters.clone(),
            data: None,
            artifacts: vec![],
            proposals: None,
            warnings: vec![],
            errors: vec![],
            evidence: Some(prepared.run.directory().display().to_string()),
        };
        prepared.persist(&outcome)?;
        let job = Arc::new(Job {
            prepared,
            outcome: Mutex::new(outcome.clone()),
            cancelled: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        });
        let task = job.clone();
        let worker = std::thread::Builder::new()
            .name(format!("operation-{}", outcome.run_id))
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(&task)));
                if result.is_err() {
                    fail(
                        &task,
                        OperationErrorCode::ToolFailure,
                        "operation worker interrupted by an internal failure".into(),
                    );
                }
            })?;
        *job.worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(worker);
        self.jobs.insert(outcome.run_id.clone(), job);
        if let Some(key) = key {
            self.requests.insert(key, (digest, outcome.run_id.clone()));
        }
        Ok(outcome)
    }
    pub fn get(&self, id: &str) -> Result<OperationOutcome> {
        let job = self
            .jobs
            .get(id)
            .ok_or_else(|| Error::Protocol("unknown job in this session".into()))?;
        Ok(job
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }
    pub fn cancel(&self, id: &str) -> Result<OperationOutcome> {
        let job = self
            .jobs
            .get(id)
            .ok_or_else(|| Error::Protocol("unknown job in this session".into()))?;
        let mut outcome = job
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Finalization owns the commit boundary; a late cancel cannot invent a
        // cancelled scientific result after publication has begun.
        if !outcome.state.terminal() && outcome.state != RuntimeState::Finalizing {
            job.cancelled.store(true, Ordering::Release);
            basecraft_io::write_atomic(
                std::path::Path::new(&job.prepared.context.cancel_file),
                |w| std::io::Write::write_all(w, b"cancel\n"),
            )
            .map_err(basecraft_ops::Error::from)?;
            outcome.state = RuntimeState::Cancelling;
            job.prepared.persist(&outcome)?;
        }
        Ok(outcome.clone())
    }
    /// Explicitly retry retained publication only; never invokes plugin code.
    pub fn repair(&self, id: &str) -> Result<OperationOutcome> {
        let job = self
            .jobs
            .get(id)
            .ok_or_else(|| Error::Protocol("unknown job in this session".into()))?;
        let mut outcome = job
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if outcome.state != RuntimeState::Partial {
            return Err(Error::Protocol(
                "only a partial publication can be repaired".into(),
            ));
        }
        let result = serde_json::from_slice(&basecraft_ops::provenance::read_record(
            &job.prepared.run.artifact("validated.json")?,
        )?)?;
        let artifacts = serde_json::from_slice::<Vec<(String, std::path::PathBuf, String)>>(
            &basecraft_ops::provenance::read_record(
                &job.prepared.run.artifact("validated-artifacts.json")?,
            )?,
        )?;
        outcome.state = RuntimeState::Finalizing;
        let mut repaired = outcome.clone();
        drop(outcome);
        let result = job.prepared.finalize(&result, &artifacts, &mut repaired);
        repaired.state = if result.is_ok() {
            RuntimeState::Completed
        } else {
            RuntimeState::Partial
        };
        if let Err(error) = result {
            repaired
                .warnings
                .push(format!("Repair remains incomplete: {error}"));
        }
        job.prepared.persist(&repaired)?;
        *job.outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = repaired.clone();
        Ok(repaired)
    }
    pub fn result(&self, id: &str) -> Result<OperationOutcome> {
        let outcome = self.get(id)?;
        if !outcome.state.terminal() {
            return Err(Error::Protocol(
                "job has not reached a terminal outcome".into(),
            ));
        }
        Ok(outcome)
    }
}
impl Drop for JobManager {
    fn drop(&mut self) {
        for id in self.jobs.keys() {
            let _ = self.cancel(id);
        }
        for job in self.jobs.values() {
            if let Some(worker) = job
                .worker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = worker.join();
            }
        }
    }
}

fn fail(job: &Job, code: OperationErrorCode, message: String) {
    let mut outcome = job
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    outcome.state = if job.cancelled.load(Ordering::Acquire) {
        RuntimeState::Cancelled
    } else if !outcome.artifacts.is_empty() || outcome.state == RuntimeState::Finalizing {
        RuntimeState::Partial
    } else {
        RuntimeState::Failed
    };
    let operation = outcome.operation.clone();
    let run_id = outcome.run_id.clone();
    let evidence = outcome.evidence.clone();
    outcome.errors.push(OperationError {
        code,
        message,
        field: None,
        operation,
        run_id: Some(run_id),
        evidence,
    });
    if let Err(error) = job.prepared.persist(&outcome) {
        outcome.errors.push(OperationError {
            code: OperationErrorCode::EvidenceIncomplete,
            message: error.to_string(),
            field: None,
            operation: outcome.operation.clone(),
            run_id: Some(outcome.run_id.clone()),
            evidence: outcome.evidence.clone(),
        });
    }
}

fn execute(job: &Job) {
    {
        let mut outcome = job
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if job.cancelled.load(Ordering::Acquire) {
            drop(outcome);
            fail(
                job,
                OperationErrorCode::Cancelled,
                "cancelled before spawn".into(),
            );
            return;
        }
        outcome.state = RuntimeState::Running;
        if let Err(error) = job.prepared.persist(&outcome) {
            drop(outcome);
            fail(
                job,
                OperationErrorCode::EvidenceIncomplete,
                error.to_string(),
            );
            return;
        }
    }
    let result = match job.prepared.execute(job.cancelled.clone()) {
        Ok(result) => result,
        Err(error) => {
            fail(job, OperationErrorCode::ToolFailure, error.to_string());
            return;
        }
    };
    let validated = match job.prepared.validate_result(&result) {
        Ok(validated) => validated,
        Err(error) => {
            fail(
                job,
                OperationErrorCode::PluginProtocolViolation,
                error.to_string(),
            );
            return;
        }
    };
    let mut outcome = job
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if job.cancelled.load(Ordering::Acquire) {
        drop(outcome);
        fail(
            job,
            OperationErrorCode::Cancelled,
            "cancelled before publication".into(),
        );
        return;
    }
    outcome.state = RuntimeState::Finalizing;
    let mut final_outcome = outcome.clone();
    drop(outcome);
    let result = job.prepared.persist(&final_outcome).and_then(|()| {
        job.prepared
            .finalize(&result, &validated, &mut final_outcome)
    });
    *job.outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = final_outcome;
    if let Err(error) = result {
        fail(
            job,
            OperationErrorCode::PublicationIncomplete,
            error.to_string(),
        );
        return;
    }
    let mut outcome = job
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    outcome.state = RuntimeState::Completed;
    if let Err(error) = job.prepared.persist(&outcome) {
        drop(outcome);
        fail(
            job,
            OperationErrorCode::EvidenceIncomplete,
            error.to_string(),
        );
    }
}

/// A known-operation adapter error uses the same structured error vocabulary.
pub fn error_value(
    operation: &str,
    code: OperationErrorCode,
    message: impl Into<String>,
) -> serde_json::Value {
    json!({"schema":1,"operation":operation,"state":"failed","errors":[OperationError {code,message:message.into(),field:None,operation:operation.into(),run_id:None,evidence:None}]})
}

pub fn error_code(error: &Error) -> OperationErrorCode {
    let message = error.to_string();
    if message.contains("permission_denied") {
        OperationErrorCode::PermissionDenied
    } else if message.contains("plugin_disabled") {
        OperationErrorCode::PluginDisabled
    } else if message.contains("plugin_changed") {
        OperationErrorCode::PluginChanged
    } else if message.contains("plugin_unavailable") {
        OperationErrorCode::PluginUnavailable
    } else if message.contains("output_conflict") {
        OperationErrorCode::OutputConflict
    } else if message.contains("stale_revision") {
        OperationErrorCode::StaleRevision
    } else if message.contains("unsupported_input") {
        OperationErrorCode::UnsupportedInput
    } else {
        OperationErrorCode::InvalidArguments
    }
}
