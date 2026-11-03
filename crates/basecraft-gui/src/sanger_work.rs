//! Sanger continuations run outside Qt. Scientific execution and receipts belong
//! exclusively to the registry/JobManager; this module only adapts review state.
use super::{
    Arc, Path, PathBuf, ReviewOp, SangerDraft, SangerReviewSession, SangerReviewView, session_view,
};
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::{OperationOutcome, RuntimeState};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// Explicit request evidence uses the shared preserve-copy ledger. The returned
/// verified path outlives source-controller snapshots, failed stages and retries.
/// Capture occurs before dispatch: a worker must never race snapshot disposal.
pub(super) fn retain_reference(anchor: &Path, source: &Path) -> Result<PathBuf, String> {
    (|| -> basecraft_ops::Result<PathBuf> {
        // A retry already names shared retained evidence; verify and reuse it.
        if source
            .file_name()
            .is_some_and(|name| name == "preserved-input")
            && let Some(directory) = source.parent()
            && directory.join("input.json").is_file()
        {
            let input = basecraft_ops::input_evidence::read_input(directory)?;
            if let Some(reference) = input.source.as_ref() {
                return basecraft_ops::input_evidence::verified_source(directory, reference);
            }
        }
        let hash = basecraft_io::hash_file(source)?;
        let run = basecraft_ops::input_evidence::reference_input_with_dependencies(
            anchor,
            source,
            &hash,
            &[],
            "GUI Sanger reference",
        )?;
        basecraft_ops::input_evidence::preserve_input(run.directory())?;
        let input = basecraft_ops::input_evidence::read_input(run.directory())?;
        let reference = input
            .source
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Missing reference evidence"))?;
        basecraft_ops::input_evidence::verified_source(run.directory(), reference)
    })()
    .map_err(|e| e.to_string())
}

pub(super) enum Request {
    Prepare(Vec<PathBuf>, u32),
    Orient(PathBuf),
    Place(PathBuf),
    Single(u32),
    Primers(serde_json::Value),
}
impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Prepare(..) => "prepare",
            Self::Orient(_) => "orient",
            Self::Place(_) => "place",
            Self::Single(_) => "single",
            Self::Primers(_) => "primers",
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            Self::Prepare(..) | Self::Single(_) => "Processing AB1 with Tracy",
            Self::Orient(_) | Self::Place(_) => "Aligning with MAFFT",
            Self::Primers(_) => "Finding primer candidates",
        }
    }
}
pub(super) struct Completion {
    pub session: SangerReviewSession,
    pub view: SangerReviewView,
    pub path: PathBuf,
    pub payload: String,
}
pub(super) struct Stage {
    pub name: &'static str,
    pub receiver: mpsc::Receiver<Result<Completion, String>>,
    pub cancelled: Arc<AtomicBool>,
}
impl Drop for Stage {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Sanger operation cancelled; previous review preserved".into())
    } else {
        Ok(())
    }
}
fn operation(
    jobs: &mut JobManager,
    root: &Path,
    operation: &str,
    inputs: serde_json::Value,
    parameters: serde_json::Value,
    cancelled: &AtomicBool,
) -> Result<OperationOutcome, String> {
    check(cancelled)?;
    let registry = RegistryConfig::discover()
        .and_then(|config| Registry::load(&config))
        .map_err(|e| e.to_string())?;
    let mut request = serde_json::json!({"operation": operation});
    request["inputs"] = inputs;
    request["parameters"] = parameters;
    let request = serde_json::from_value(request).map_err(|e| e.to_string())?;
    let policy = SessionPolicy::new(root, true).map_err(|e| e.to_string())?;
    let accepted = jobs
        .submit(&registry, request, policy, "GUI")
        .map_err(|e| e.to_string())?;
    loop {
        if cancelled.load(Ordering::Acquire) {
            jobs.cancel(&accepted.run_id).map_err(|e| e.to_string())?;
        }
        let outcome = jobs.get(&accepted.run_id).map_err(|e| e.to_string())?;
        if outcome.state.terminal() {
            check(cancelled)?;
            if outcome.state != RuntimeState::Completed {
                return Err(format!(
                    "{:?}: {}. Retained evidence: {}",
                    outcome.state,
                    outcome
                        .errors
                        .iter()
                        .map(|e| e.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                    outcome.evidence.as_deref().unwrap_or("unavailable")
                ));
            }
            return Ok(outcome);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
fn artifact(outcome: &OperationOutcome, name: &str) -> Result<PathBuf, String> {
    outcome
        .artifacts
        .iter()
        .find(|a| Path::new(&a.path).file_name().is_some_and(|n| n == name))
        .map(|a| PathBuf::from(&a.path))
        .ok_or_else(|| format!("Operation did not retain {name}"))
}
fn directory(outcome: &OperationOutcome) -> Result<&Path, String> {
    outcome
        .evidence
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "Operation receipt has no evidence directory".into())
}
fn evaluate(session: &mut SangerReviewSession, view: &mut SangerReviewView) -> Result<(), String> {
    view.review_settings = session.settings.clone();
    session.apply(ReviewOp::Evaluate {
        version: 2,
        settings: session.settings.clone(),
        calls: (0..view.draft.columns.len())
            .map(|i| view.assessment(i).call)
            .collect(),
        reference: view.frozen_reference(),
    })
}

pub(super) fn start(
    request: Request,
    session: Option<SangerReviewSession>,
) -> Result<Stage, String> {
    let name = request.name();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("sanger-{name}"))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(request, session, &flag)
            }))
            .unwrap_or_else(|_| {
                Err("Sanger continuation interrupted; previous review preserved".into())
            });
            let _ = sender.send(result);
        })
        .map_err(|e| e.to_string())?;
    Ok(Stage {
        name,
        receiver,
        cancelled,
    })
}
fn execute(
    request: Request,
    previous: Option<SangerReviewSession>,
    cancelled: &AtomicBool,
) -> Result<Completion, String> {
    let mut jobs = JobManager::default().with_task_cancellation();
    let mut payload = String::new();
    let (mut session, path) = match request {
        Request::Prepare(paths, threshold) => {
            let first = paths.first().ok_or("Choose two or three AB1 reads")?;
            let root = first.parent().ok_or("AB1 has no parent directory")?;
            let inputs: Vec<_> = paths
                .iter()
                .map(|path| serde_json::json!({"kind":"file","path":path}))
                .collect();
            let outcome = operation(
                &mut jobs,
                root,
                "basecraft.tracy-sanger/consensus@1",
                serde_json::json!({"reads":inputs}),
                serde_json::json!({"signal_threshold":threshold}),
                cancelled,
            )?;
            let path = artifact(&outcome, "evidence.json")?;
            let mut session = SangerReviewSession::from_draft(SangerDraft::read(&path)?, first)?;
            let mut view = session_view(&session)?;
            evaluate(&mut session, &mut view)?;
            session.attach_operation_run(directory(&outcome)?)?;
            (session, path)
        }
        request => {
            let mut session = previous.ok_or("Open a Sanger review first")?;
            let path = session.storage_anchor().to_path_buf();
            let root = path.parent().ok_or("Review has no parent directory")?;
            match request {
                Request::Single(threshold) => {
                    let draft = session.working_draft()?;
                    if draft.inputs.len() != 1 {
                        return Err("Choose one trace to compute calls".into());
                    }
                    let source = &draft.inputs[0];
                    let mut caller = basecraft_plugin_host::sanger_calls::SangerCaller::default()
                        .with_task_cancellation();
                    let mut called = caller.start(&source.path, &source.sha256, threshold)?;
                    while called.is_none() {
                        if cancelled.load(Ordering::Acquire) {
                            caller.cancel()?;
                            check(cancelled)?;
                        }
                        called = caller.poll()?;
                        if called.is_none() {
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                    }
                    let called = called.ok_or("Caller returned no draft")?;
                    let mut alternate =
                        SangerReviewSession::from_draft(called.draft, &source.path)?;
                    alternate.attach_operation_run(&called.run)?;
                    session.retain_call_set(alternate)?;
                }
                Request::Orient(alignment) => {
                    let draft = session.working_draft()?;
                    let outcome = operation(
                        &mut jobs,
                        root,
                        "basecraft.mafft/add_sequences@1",
                        serde_json::json!({"alignment":[{"kind":"file","path":alignment}]}),
                        serde_json::json!({"rows":[["probe",draft.consensus.replace('-',"")]],"orientation":true,"keep":true}),
                        cancelled,
                    )?;
                    let value: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(artifact(&outcome, "addition.json")?)
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    if value["reversed"].as_array().is_some_and(|a| !a.is_empty()) {
                        if !session.automatic_only() {
                            return Err("This review already has decisions; open a fresh draft to change orientation".into());
                        }
                        let mut draft = session.working_draft()?.reverse_complemented();
                        draft.limitations.push(format!("Draft reverse-complemented for alignment context; source draft SHA-256 {}",session.draft_sha256));
                        let mut oriented = SangerReviewSession::from_draft(draft, &path)?;
                        oriented.inherit_review_ancestry(&session);
                        oriented.settings = session.settings.clone();
                        oriented.attach_operation_run(directory(&outcome)?)?;
                        if session.draft.inputs.len() == 1 {
                            let id = oriented.draft_sha256.clone();
                            session.retain_call_set(oriented)?;
                            session.select_call_set(&id)?;
                        } else {
                            session = oriented;
                        }
                    } else {
                        session.attach_operation_run(directory(&outcome)?)?;
                    }
                }
                Request::Place(alignment) => {
                    let outcome = operation(
                        &mut jobs,
                        root,
                        "basecraft.mafft/place_sanger@1",
                        serde_json::json!({"alignment":[{"kind":"file","path":alignment}]}),
                        serde_json::json!({"fragment":session.draft.consensus.replace('-',"")}),
                        cancelled,
                    )?;
                    session.attach_operation_placement(
                        directory(&outcome)?,
                        &artifact(&outcome, "placement.json")?,
                    )?;
                    let mut view = session_view(&session)?;
                    let automatic = session.automatic_only();
                    evaluate(&mut session, &mut view)?;
                    if automatic && let Some((start, end)) = view.reference_mask_bounds() {
                        let mut settings = session.mask.clone();
                        settings.start = start;
                        settings.end = end;
                        session.apply(ReviewOp::Mask { settings })?;
                    }
                }
                Request::Primers(mut parameters) => {
                    let view = session_view(&session)?;
                    parameters["sequence"] = serde_json::json!(
                        view.consensus
                            .iter()
                            .filter(|&&b| b != b'-')
                            .map(|&b| char::from(b))
                            .collect::<String>()
                    );
                    let outcome = operation(
                        &mut jobs,
                        root,
                        "basecraft.tracy-sanger/primers@1",
                        serde_json::json!({}),
                        parameters,
                        cancelled,
                    )?;
                    payload = std::fs::read_to_string(artifact(&outcome, "primers.json")?)
                        .map_err(|e| e.to_string())?;
                    session.attach_operation_run(directory(&outcome)?)?;
                }
                Request::Prepare(..) => return Err("Unexpected nested preparation request".into()),
            }
            (session, path)
        }
    };
    check(cancelled)?;
    let view = session_view(&session)?;
    session.start_recovery()?;
    check(cancelled)?;
    Ok(Completion {
        session,
        view,
        path,
        payload,
    })
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // harness=false application_bootstrap includes this module.
    use super::*;

    #[test]
    fn cancelled_stage_does_not_prepare_or_replace_review() {
        let flag = AtomicBool::new(true);
        let result = execute(
            Request::Prepare(
                vec![PathBuf::from("missing.ab1"), PathBuf::from("missing-2.ab1")],
                20,
            ),
            None,
            &flag,
        );
        assert!(result.err().unwrap().contains("cancelled"));
    }

    #[test]
    #[ignore = "requires BASECRAFT_SANGER_PAIR_DIR with two AB1 reads, full reference, Tracy and MAFFT"]
    fn real_pair_full_reference_survives_temporary_input_deletion() {
        let fixture = PathBuf::from(std::env::var_os("BASECRAFT_SANGER_PAIR_DIR").unwrap());
        let temp = tempfile::tempdir().unwrap();
        let mut reads = Vec::new();
        let mut reference = None;
        for entry in std::fs::read_dir(fixture).unwrap() {
            let path = entry.unwrap().path();
            let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(extension, "ab1" | "fasta") {
                continue;
            }
            let target = temp.path().join(path.file_name().unwrap());
            std::fs::copy(&path, &target).unwrap();
            if extension == "ab1" {
                reads.push(target);
            } else {
                reference = Some(target);
            }
        }
        reads.sort();
        assert_eq!(reads.len(), 2);
        let reference = reference.unwrap();
        let original_hash = basecraft_io::hash_file(&reference).unwrap();
        let frozen = retain_reference(&reads[0], &reference).unwrap();
        let flag = AtomicBool::new(false);
        let preparation = start(Request::Prepare(reads.clone(), 20), None).unwrap();
        // Dispose the alignment-owned snapshot before observing first completion.
        std::fs::remove_file(&reference).unwrap();
        assert_eq!(basecraft_io::hash_file(&frozen).unwrap(), original_hash);
        assert_eq!(retain_reference(&reads[0], &frozen).unwrap(), frozen);
        let prepared = preparation
            .receiver
            .recv()
            .unwrap()
            .unwrap_or_else(|e| panic!("prepare: {e}"));
        assert_eq!(prepared.session.draft.inputs.len(), 2);
        let oriented = execute(
            Request::Orient(frozen.clone()),
            Some(prepared.session),
            &flag,
        )
        .unwrap_or_else(|e| panic!("orient: {e}"));
        // A replacement source between stages must not change the request.
        std::fs::write(&reference, ">replacement\nAAAA\n").unwrap();
        assert_eq!(basecraft_io::hash_file(&frozen).unwrap(), original_hash);
        let mut placed = execute(
            Request::Place(frozen.clone()),
            Some(oriented.session),
            &flag,
        )
        .unwrap_or_else(|e| panic!("place: {e}"));
        assert!(
            placed.session.unresolved() > 0,
            "review must not auto-accept conflicts"
        );
        assert!(placed.session.placement().unwrap().unwrap().source_rows > 1000);
        for source in reads.into_iter().chain([reference]) {
            std::fs::remove_file(source).unwrap();
        }
        let saved = temp.path().join("saved-review.json");
        placed.session.save(&saved).unwrap();
        let reopened = SangerReviewSession::load(&saved).unwrap();
        let view = session_view(&reopened).unwrap();
        assert_eq!(view.draft.consensus, placed.view.draft.consensus);
        assert_eq!(reopened.unresolved(), placed.session.unresolved());
        let placement = reopened.placement().unwrap().unwrap();
        assert_eq!(
            basecraft_io::hash_file(&placement.source_path).unwrap(),
            original_hash
        );
        assert_eq!(
            reopened.draft.columns.len(),
            placed.session.draft.columns.len()
        );
    }
}
