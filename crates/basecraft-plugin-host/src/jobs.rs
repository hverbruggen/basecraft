//! Process/session-owned jobs. Accepted runs never depend on a wait request's
//! lifetime; explicit cancellation and manager drop own worker teardown.
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

use crate::{Error, Result, policy::SessionPolicy, registry::Registry, runner::PreparedOperation};
use basecraft_protocol::{
    operation::{
        OperationError, OperationErrorCode, OperationOutcome, OperationRequest, ProposalOutcome,
        RuntimeState,
    },
    plugin::InvokeResult,
};
use serde_json::json;

struct PendingProposals {
    result: InvokeResult,
    validated: Vec<(String, std::path::PathBuf, String)>,
}

#[derive(Default, PartialEq)]
enum TerminalHistory {
    #[default]
    Pending,
    Recorded,
    Published,
}

#[derive(Default)]
struct DocumentFinalization {
    activity: bool,
    applied: bool,
    saved: bool,
    terminal_history: TerminalHistory,
    generation: Option<u64>,
    edits: Option<Vec<basecraft_protocol::delimitation::DelimitationEdit>>,
    next_edit: usize,
    undo_group: Option<u64>,
    expected_revision: Option<u64>,
}

struct Job {
    prepared: PreparedOperation,
    outcome: Mutex<OperationOutcome>,
    cancelled: Arc<AtomicBool>,
    // Proposal activity is an owner-session journal write, separate from file
    // publication. A publication-only repair cannot recreate a missing entry.
    proposal_activity_recorded: AtomicBool,
    // Set once a document-bound run is published; the owning session then
    // records it in the document's history and completes the job.
    document_pending: AtomicBool,
    // A document-bound run logs its terminal state in that document's history once.
    document_finished: AtomicBool,
    document_finalization: Mutex<DocumentFinalization>,
    started: std::time::Instant,
    worker: Mutex<Option<JoinHandle<()>>>,
    proposals: Mutex<Option<PendingProposals>>,
}

#[derive(Default)]
pub struct JobManager {
    jobs: BTreeMap<String, Arc<Job>>,
    requests: BTreeMap<String, (String, String)>,
    skip_known_features: bool,
    task_cancellation: bool,
    task_registrations: Vec<crate::file_tool::CancellationRegistration>,
}

/// A non-owning Tasks capability. Dropping the manager revokes it even while a
/// worker is being reaped. All state transitions use the same lock as cancel.
#[derive(Clone)]
pub(crate) struct TaskCancellation(Weak<Job>);

fn cancellable(state: RuntimeState) -> bool {
    matches!(state, RuntimeState::Queued | RuntimeState::Running)
}

impl TaskCancellation {
    pub(crate) fn can_cancel(&self) -> bool {
        self.0.upgrade().is_some_and(|job| {
            let outcome = job
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cancellable(outcome.state)
        })
    }

    pub(crate) fn cancel(&self) -> Result<()> {
        let job = self
            .0
            .upgrade()
            .ok_or_else(|| Error::Protocol("Cancellation is unavailable for this run".into()))?;
        cancel_job(&job, true).map(|_| ())
    }
}

/// `strict` is used by Tasks: a stale click must not claim a request succeeded
/// after the run crossed publication or reached a terminal state. The manager's
/// existing API remains idempotent for wait loops and teardown.
fn cancel_job(job: &Job, strict: bool) -> Result<OperationOutcome> {
    let mut outcome = job
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !outcome.state.terminal() && outcome.state != RuntimeState::Finalizing {
        if strict && !cancellable(outcome.state) {
            return Err(Error::Protocol(
                "Cancellation is unavailable for this run".into(),
            ));
        }
        job.cancelled.store(true, Ordering::Release);
        basecraft_io::write_atomic(
            std::path::Path::new(&job.prepared.context.cancel_file),
            |w| std::io::Write::write_all(w, b"cancel\n"),
        )
        .map_err(basecraft_ops::Error::from)?;
        outcome.state = RuntimeState::Cancelling;
        job.prepared.persist(&outcome)?;
    } else if strict {
        return Err(Error::Protocol(
            "Cancellation is unavailable for this run".into(),
        ));
    }
    Ok(outcome.clone())
}

impl JobManager {
    /// Expose this GUI owner's active jobs to the existing Tasks cancellation
    /// adapter. Headless managers remain private to their own session APIs.
    #[must_use]
    pub fn with_task_cancellation(mut self) -> Self {
        self.task_cancellation = true;
        self
    }

    pub fn submit(
        &mut self,
        registry: &Registry,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: &str,
    ) -> Result<OperationOutcome> {
        self.submit_with_session(registry, request, policy, initiator, None)
    }
    pub fn submit_in_session(
        &mut self,
        registry: &Registry,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: &str,
        session: &mut basecraft_ops::Session,
    ) -> Result<OperationOutcome> {
        self.submit_with_session(registry, request, policy, initiator, Some(session))
    }
    /// Leave out proposed features whose kind and location the target document
    /// already has, reporting each as rejected. The GUI annotation tools rerun
    /// on annotated sequences; headless routes apply every proposal.
    #[must_use]
    pub fn skipping_known_features(mut self) -> Self {
        self.skip_known_features = true;
        self
    }
    /// Drive one accepted job from its owning session thread until it is
    /// terminal: proposals are applied here, between waits. `interrupted`
    /// requests cancellation, for example on Ctrl-C.
    pub fn wait_in_session(
        &self,
        id: &str,
        session: &mut basecraft_ops::Session,
        interrupted: &dyn Fn() -> bool,
    ) -> Result<OperationOutcome> {
        loop {
            self.poll_session(session);
            if interrupted() {
                self.cancel(id)?;
            }
            let outcome = self.get(id)?;
            if outcome.state.terminal() {
                return Ok(outcome);
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    pub fn target_document(&self, id: &str) -> Option<basecraft_ops::DocId> {
        self.jobs.get(id).and_then(|job| {
            job.prepared
                .sequence
                .as_ref()
                .map(|target| target.doc)
                .or_else(|| job.prepared.document.as_ref().map(|target| target.doc))
        })
    }
    /// Revision after this run's own changes, only when the target still
    /// matched its submission. A genuine intervening edit has no receipt.
    pub fn finalized_generation(&self, id: &str) -> Option<u64> {
        self.jobs
            .get(id)?
            .document_finalization
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .generation
    }
    /// Called by the owning session thread: applies sequence proposals and
    /// records document-bound runs in their document's history. No session lock
    /// is held while a plugin runs; identity is checked at the edit boundary.
    pub fn poll_session(&self, session: &mut basecraft_ops::Session) {
        self.poll_proposals(session);
        for job in self.jobs.values() {
            let Some(target) = &job.prepared.document else {
                continue;
            };
            if target.session != session.id() {
                continue;
            }
            if job.document_pending.swap(false, Ordering::AcqRel) {
                let outcome = job
                    .outcome
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                match finalize_document(job, target, &outcome, session) {
                    Ok(()) => {
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
                    Err(error) => fail(
                        job,
                        OperationErrorCode::EvidenceIncomplete,
                        error.to_string(),
                    ),
                }
            }
            let outcome = job
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if outcome.state.terminal() && !job.document_finished.load(Ordering::Acquire) {
                match record_document_finished(job, target, &outcome, session) {
                    Ok(()) => job.document_finished.store(true, Ordering::Release),
                    Err(error) => {
                        job.document_finished.store(true, Ordering::Release);
                        fail(
                            job,
                            OperationErrorCode::EvidenceIncomplete,
                            error.to_string(),
                        );
                    }
                }
            }
        }
    }
    fn poll_proposals(&self, session: &mut basecraft_ops::Session) {
        for job in self.jobs.values() {
            if job
                .prepared
                .sequence
                .as_ref()
                .is_some_and(|target| target.session != session.id())
            {
                continue;
            }
            let pending = job
                .proposals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let Some(PendingProposals { result, validated }) = pending else {
                continue;
            };
            let Some(target) = &job.prepared.sequence else {
                continue;
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
                    "cancelled before proposal commit".into(),
                );
                continue;
            }
            outcome.state = RuntimeState::Finalizing;
            drop(outcome);
            let actor = basecraft_protocol::Actor::Plugin {
                id: job.prepared.operation.package.manifest.id.clone(),
                version: job.prepared.operation.package.manifest.version.clone(),
            };
            let mut rejected = Vec::new();
            let known = session
                .checked_sequence(target.doc)
                .ok()
                .filter(|_| self.skip_known_features);
            let ops: Vec<_> = if let Some(document) = known {
                result
                    .ops
                    .iter()
                    .filter(|op| match op {
                        basecraft_protocol::Op::FeatureAdd { feature }
                            if document.features().any(|(_, known)| {
                                known.kind == feature.kind && known.location == feature.location
                            }) =>
                        {
                            rejected.push(format!("already annotated: {}", feature.kind));
                            false
                        }
                        _ => true,
                    })
                    .cloned()
                    .collect()
            } else {
                result.ops.clone()
            };
            if let Err(error) = session.apply_sequence_proposals(
                target.doc,
                target.generation,
                target.revision,
                &ops,
                &actor,
            ) {
                job.outcome
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .state = RuntimeState::Running;
                fail(
                    job,
                    if error.to_string().contains("stale_revision") {
                        OperationErrorCode::StaleRevision
                    } else {
                        OperationErrorCode::PluginProtocolViolation
                    },
                    error.to_string(),
                );
                continue;
            }
            let mut outcome = job
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            outcome.proposals = Some(ProposalOutcome {
                document: Some(target.doc.0.to_string()),
                applied: ops.len(),
                rejected,
            });
            outcome.warnings.push(format!(
                "Proposals attached to session document {} at generation {}",
                target.doc.0,
                session.generation(target.doc)
            ));
            let snapshot_path = job.prepared.run.artifact("snapshot.arrow-pair");
            let activity = snapshot_path.and_then(|path| {
                let hash = basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?;
                let key = basecraft_ops::input_evidence::relative(session.root(),&path)?;
                let params = json!({"requested":job.prepared.request.parameters,"effective":outcome.effective_parameters,
                    "operation":job.prepared.operation.id,"pins":outcome.pins,"initiator":job.prepared.initiator,"run_id":outcome.run_id});
                session.log_tool_run(target.doc,basecraft_protocol::Actor::Plugin {id:job.prepared.operation.package.manifest.id.clone(),version:job.prepared.operation.package.manifest.version.clone()},
                    (&job.prepared.operation.package.manifest.id,&job.prepared.operation.descriptor.implementation),params,&[(key,hash)],&[])?;
                Ok(())
            });
            if let Err(error) = activity {
                drop(outcome);
                fail(
                    job,
                    OperationErrorCode::EvidenceIncomplete,
                    error.to_string(),
                );
                continue;
            }
            job.proposal_activity_recorded
                .store(true, Ordering::Release);
            let mut final_outcome = outcome.clone();
            drop(outcome);
            let finalized = job
                .prepared
                .finalize(&result, &validated, &mut final_outcome, false);
            final_outcome.state = if finalized.is_ok() {
                RuntimeState::Completed
            } else {
                RuntimeState::Partial
            };
            *job.outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = final_outcome.clone();
            if let Err(error) = finalized.and_then(|()| job.prepared.persist(&final_outcome)) {
                fail(
                    job,
                    OperationErrorCode::PublicationIncomplete,
                    error.to_string(),
                );
            }
        }
    }
    fn submit_with_session(
        &mut self,
        registry: &Registry,
        request: OperationRequest,
        policy: SessionPolicy,
        initiator: &str,
        session: Option<&mut basecraft_ops::Session>,
    ) -> Result<OperationOutcome> {
        let descriptor = request
            .request_key
            .as_ref()
            .and_then(|key| self.requests.get(key))
            .and_then(|(_, id)| self.jobs.get(id))
            .map(|job| &job.prepared.operation.descriptor)
            .or_else(|| {
                registry
                    .operations
                    .get(&request.operation)
                    .map(|op| &op.descriptor)
            });
        let mut normalized = request.clone();
        if let Some(descriptor) = descriptor {
            normalized.parameters =
                crate::contract::normalize_parameters(descriptor, &request.parameters)?;
        }
        let digest = basecraft_io::hash_bytes(&serde_json::to_vec(&normalized)?);
        if let Some(key) = &request.request_key
            && let Some((existing, id)) = self.requests.get(key)
        {
            if existing != &digest {
                return Err(Error::Protocol(
                    "invalid_arguments: request key reused with different inputs".into(),
                ));
            }
            return self.get(id);
        }
        if self.jobs.len() >= 128 {
            return Err(Error::Protocol("session job limit reached".into()));
        }
        let operation = registry
            .resolve(&request.operation, request.pins.as_ref())?
            .clone();
        let pins = operation.pins(&registry.generation);
        let key = request.request_key.clone();
        let prepared = PreparedOperation::prepare(
            operation,
            request,
            policy,
            initiator.into(),
            session,
            registry,
        )?;
        let outcome = OperationOutcome {
            schema: 1,
            operation: prepared.operation.id.clone(),
            run_id: prepared.run.id().into(),
            state: RuntimeState::Queued,
            pins,
            requested_parameters: prepared.request.parameters.clone(),
            effective_parameters: prepared.parameters.clone(),
            warnings: prepared.input_diagnostics.clone(),
            data: None,
            artifacts: vec![],
            proposals: None,
            errors: vec![],
            evidence: Some(prepared.run.directory().display().to_string()),
        };
        prepared.persist(&outcome)?;
        let job = Arc::new(Job {
            prepared,
            outcome: Mutex::new(outcome.clone()),
            cancelled: Arc::new(AtomicBool::new(false)),
            proposal_activity_recorded: AtomicBool::new(false),
            document_pending: AtomicBool::new(false),
            document_finished: AtomicBool::new(false),
            document_finalization: Mutex::new(DocumentFinalization::default()),
            started: std::time::Instant::now(),
            worker: Mutex::new(None),
            proposals: Mutex::new(None),
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
        if self.task_cancellation {
            self.task_registrations
                .push(crate::file_tool::register_task_cancellation(
                    outcome.run_id.clone(),
                    TaskCancellation(Arc::downgrade(&job)),
                ));
        }
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
        cancel_job(job, false)
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
        if job.prepared.sequence.is_some()
            && !job.proposal_activity_recorded.load(Ordering::Acquire)
        {
            return Err(Error::Protocol(
                "evidence_incomplete: proposal tool-run activity was not recorded; publication-only repair cannot restore the owning session journal".into(),
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
        let result = job
            .prepared
            .finalize(&result, &artifacts, &mut repaired, true);
        repaired.state = if result.is_ok() && job.prepared.document.is_some() {
            job.document_pending.store(true, Ordering::Release);
            RuntimeState::Finalizing
        } else if result.is_ok() {
            RuntimeState::Completed
        } else {
            RuntimeState::Partial
        };
        if let Err(error) = result {
            repaired
                .warnings
                .push(format!("Repair remains incomplete: {error}"));
        }
        let persisted = job.prepared.persist(&repaired);
        *job.outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = repaired;
        if let Err(error) = persisted {
            fail(
                job,
                OperationErrorCode::EvidenceIncomplete,
                error.to_string(),
            );
        }
        self.get(id)
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
        self.task_registrations.clear();
        for id in self.jobs.keys() {
            let _ = self.cancel(id);
        }
        for job in self.jobs.values() {
            if job
                .proposals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .is_some()
            {
                fail(
                    job,
                    OperationErrorCode::Cancelled,
                    "session closed before proposal commit".into(),
                );
            }
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
    outcome.effective_parameters = job.prepared.effective_parameters();
    outcome.state = if !outcome.artifacts.is_empty()
        || outcome
            .proposals
            .as_ref()
            .is_some_and(|receipt| receipt.applied > 0)
        || matches!(
            outcome.state,
            RuntimeState::Finalizing | RuntimeState::Partial | RuntimeState::Completed
        ) {
        RuntimeState::Partial
    } else if job.cancelled.load(Ordering::Acquire) {
        if job.prepared.lifecycle.external() && code != OperationErrorCode::Cancelled {
            RuntimeState::Interrupted
        } else {
            RuntimeState::Cancelled
        }
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
        let error = OperationError {
            code: OperationErrorCode::EvidenceIncomplete,
            message: error.to_string(),
            field: None,
            operation: outcome.operation.clone(),
            run_id: Some(outcome.run_id.clone()),
            evidence: outcome.evidence.clone(),
        };
        outcome.errors.push(error);
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
    job.outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .effective_parameters = job.prepared.effective_parameters();
    if job.prepared.lifecycle.external() {
        if let Err(error) = job.prepared.submission_identity(&result) {
            fail(
                job,
                OperationErrorCode::PluginProtocolViolation,
                error.to_string(),
            );
            return;
        }
        let mut outcome = job
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        outcome.data = result.data.as_deref().cloned();
        outcome.state = RuntimeState::Finalizing;
        if let Some(message) = &result.message {
            outcome.warnings.push(message.clone());
        }
    }
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
    if job.prepared.sequence.is_some() {
        *job.proposals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(PendingProposals { result, validated });
        return;
    }
    let mut outcome = job
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if job.prepared.lifecycle.external() {
        // Preserve the validated remote identity before local publication. Late
        // cancellation is partial, repairable locally, never an external undo.
        outcome.data = result.data.as_deref().cloned();
        outcome.state = RuntimeState::Finalizing;
        if job.cancelled.load(Ordering::Acquire) {
            drop(outcome);
            fail(job, OperationErrorCode::Cancelled,
                "Cancellation cannot undo an external submission; inspect retained receipt, repair publication only".into());
            return;
        }
    } else if job.cancelled.load(Ordering::Acquire) {
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
    let publication = job.prepared.persist(&final_outcome).and_then(|()| {
        job.prepared
            .finalize(&result, &validated, &mut final_outcome, false)
    });
    *job.outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = final_outcome;
    if let Err(error) = publication {
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
    outcome.state = if job.prepared.lifecycle.external() && result.message.is_some() {
        RuntimeState::Partial
    } else if job.prepared.document.is_some() {
        job.document_pending.store(true, Ordering::Release);
        RuntimeState::Finalizing
    } else {
        RuntimeState::Completed
    };
    if let Err(error) = job.prepared.persist(&outcome) {
        drop(outcome);
        fail(
            job,
            OperationErrorCode::EvidenceIncomplete,
            error.to_string(),
        );
    }
}

fn same_document(session: &basecraft_ops::Session, target: &crate::runner::DocumentTarget) -> bool {
    session.id() == target.session
        && session.is_open(target.doc)
        && session.root().join(session.doc_key(target.doc)) == target.path
}

/// Owner-thread continuation used by both execution and publication repair.
/// A failed save/publication never repeats already applied typed edits.
fn finalize_document(
    job: &Job,
    target: &crate::runner::DocumentTarget,
    outcome: &OperationOutcome,
    session: &mut basecraft_ops::Session,
) -> Result<()> {
    if !same_document(session, target) {
        return Err(Error::Protocol(
            "input document was closed or replaced".into(),
        ));
    }
    let mut progress = job
        .document_finalization
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !progress.activity {
        // A failed receipt write after a successful journal append must not
        // invent a second producing activity on repair.
        let recorded = session
            .document_record(target.doc)?
            .events
            .iter()
            .any(|event| {
                matches!(&event.op, basecraft_protocol::Op::ToolRun { params, .. }
                if params["run_id"].as_str() == Some(outcome.run_id.as_str()))
            });
        if !recorded {
            record_document_run(job, target, outcome, session)?;
        }
        progress.activity = true;
    }
    let mut snapshot = job.prepared.input_record.clone();
    // A receiving workspace is an edit destination, not an analysis input.
    // Making the result depend on it would cycle when that workspace records
    // the newly attached result as one of its own dependencies.
    if !target.receives_results {
        snapshot.include_input(&target.record);
    }
    session.complete_operation_publication(target.doc, &snapshot, &job.prepared.run)?;
    if !progress.applied {
        let unchanged = if let Some(revision) = progress.expected_revision {
            session.revision(target.doc) == revision
        } else {
            session.generation(target.doc) == target.generation
                && session.revision(target.doc) == target.revision
        };
        let warning = apply_target_results(job, target, outcome, session, &mut progress)?;
        progress.applied = true;
        if unchanged {
            progress.generation = Some(session.generation(target.doc));
        }
        if let Some(warning) = warning {
            job.outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .warnings
                .push(warning);
        }
    }
    if target.save && !progress.saved {
        if let Some(generation) = progress.generation {
            if session.generation(target.doc) != generation {
                return Err(Error::Protocol("stale_revision: target changed after result attachment; save remains incomplete".into()));
            }
            session.save(target.doc, None)?;
        }
        progress.saved = true;
    }
    if progress.terminal_history == TerminalHistory::Pending {
        let mut completed = outcome.clone();
        completed.state = RuntimeState::Completed;
        record_document_finished(job, target, &completed, session)?;
        progress.terminal_history = TerminalHistory::Recorded;
    }
    // A successful append is retried only as publication, never as another
    // terminal activity, even if its saved report could not be written yet.
    job.document_finished.store(true, Ordering::Release);
    if target.save
        && progress.generation.is_some()
        && progress.terminal_history != TerminalHistory::Published
    {
        if progress.generation != Some(session.generation(target.doc)) {
            return Err(Error::Protocol(
                "stale_revision: target changed before terminal history publication".into(),
            ));
        }
        session.save_history(target.doc)?;
        progress.terminal_history = TerminalHistory::Published;
    }
    Ok(())
}

/// The history entry for a published run that read a session document. The
/// run's own publication already holds the full evidence; this links the
/// document's history to it by run ID and records the submitted input.
fn record_document_run(
    job: &Job,
    target: &crate::runner::DocumentTarget,
    outcome: &OperationOutcome,
    session: &mut basecraft_ops::Session,
) -> Result<()> {
    if !same_document(session, target) {
        return Err(Error::Protocol(
            "input document was closed or replaced before the run was recorded".into(),
        ));
    }
    let manifest = &job.prepared.operation.package.manifest;
    let (actor, params) = job.prepared.tool_activity(outcome)?;
    let outputs: Vec<_> = outcome
        .artifacts
        .iter()
        .map(|artifact| std::path::PathBuf::from(&artifact.path))
        .collect();
    let operation = (
        manifest.id.as_str(),
        job.prepared.operation.descriptor.implementation.as_str(),
    );
    let inputs = std::slice::from_ref(&target.input);
    if target.saved.is_some() {
        session.log_referenced_tool_run(target.doc, actor, operation, params, inputs, &outputs)?;
    } else {
        session.log_tool_run(target.doc, actor, operation, params, inputs, &outputs)?;
    }
    Ok(())
}

/// A species workspace named as the run's target receives its hypotheses,
/// unless it changed while the run worked; then the result stays an artifact.
fn apply_target_results(
    job: &Job,
    target: &crate::runner::DocumentTarget,
    outcome: &OperationOutcome,
    session: &mut basecraft_ops::Session,
    progress: &mut DocumentFinalization,
) -> Result<Option<String>> {
    if !target.receives_results {
        return Ok(None);
    }
    let mut warning = None;
    // Only species analyses add hypotheses; a display tree run with a
    // workspace target is recorded in its history and nothing more.
    let species = matches!(
        job.prepared.operation.package.manifest.id.as_str(),
        "basecraft.delimitation" | "basecraft.host"
    );
    if species && session.is_delimitation(target.doc) {
        if session.revision(target.doc) == progress.expected_revision.unwrap_or(target.revision) {
            let name = job.prepared.species_result_name();
            if let Some(result) = outcome
                .artifacts
                .iter()
                .map(|artifact| std::path::PathBuf::from(&artifact.path))
                .find(|path| path.file_name().is_some_and(|file| file == name.as_str()))
            {
                if progress.edits.is_none() {
                    let (_, edits) = basecraft_ops::delimitation::analysis_result_edits(
                        session, target.doc, &result,
                    )?;
                    progress.edits = Some(edits);
                }
                if let Some(edits) = &progress.edits {
                    if progress.next_edit == 0 {
                        progress.undo_group = Some(session.begin_group());
                    } else if let Some(group) = progress.undo_group {
                        session.resume_group(target.doc, group)?;
                    } else {
                        return Err(Error::Protocol("missing result undo group".into()));
                    }
                    let applied = (|| -> Result<()> {
                        for edit in &edits[progress.next_edit..] {
                            session.apply(
                                target.doc,
                                basecraft_protocol::Op::DelimitationEdit {
                                    edit: Box::new(edit.clone()),
                                },
                            )?;
                            progress.next_edit += 1;
                            progress.expected_revision = Some(session.revision(target.doc));
                        }
                        Ok(())
                    })();
                    session.end_group();
                    applied?;
                }
            }
        } else {
            warning =
                Some("Workspace changed during the run; its result remains an artifact".into());
        }
    }
    Ok(warning)
}

/// The terminal state of a run that read a session document, with its outputs
/// and diagnostics, as the document history records every analysis attempt.
fn record_document_finished(
    job: &Job,
    target: &crate::runner::DocumentTarget,
    outcome: &OperationOutcome,
    session: &mut basecraft_ops::Session,
) -> Result<()> {
    use basecraft_protocol::delimitation::{JobRecord, JobStatus};
    if !same_document(session, target) {
        return Err(Error::Protocol(
            "input document was closed or replaced".into(),
        ));
    }
    let report = JobRecord {
        id: outcome.run_id.clone(),
        generation: target.generation,
        status: match outcome.state {
            RuntimeState::Completed => JobStatus::Completed,
            RuntimeState::Cancelled => JobStatus::Cancelled,
            _ => JobStatus::Failed,
        },
        method: job.prepared.operation.descriptor.implementation.clone(),
        inputs: std::iter::once(target.input.clone()).collect(),
        outputs: outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), artifact.digest.clone()))
            .collect(),
        diagnostics: outcome
            .errors
            .iter()
            .map(|error| error.message.clone())
            .collect(),
        elapsed_seconds: job.started.elapsed().as_secs_f64(),
    };
    if target.saved.is_some() {
        session.log_referenced_job_finished(target.doc, report)?;
    } else {
        session.log_job_finished(target.doc, report)?;
    }
    Ok(())
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
    } else if message.contains("evidence_incomplete") {
        OperationErrorCode::EvidenceIncomplete
    } else if message.contains("unsupported_input") {
        OperationErrorCode::UnsupportedInput
    } else {
        OperationErrorCode::InvalidArguments
    }
}
