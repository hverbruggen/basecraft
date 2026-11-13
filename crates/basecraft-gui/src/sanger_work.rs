//! Sanger continuations run outside Qt. Scientific execution and receipts belong
//! exclusively to the registry/JobManager; this module only adapts review state.
use super::{Path, PathBuf, ReviewChoice, ReviewOp, SangerReview, SangerReviewView};
use basecraft_docs::{
    AlignmentDoc,
    sanger::{SangerDraft, SangerDraftExt},
};
use basecraft_io::abif;
use basecraft_plugin_host::{jobs::JobManager, policy::SessionPolicy};
use basecraft_protocol::operation::{OperationOutcome, RuntimeState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
            let input = basecraft_ops::records::read_input(directory)?;
            if let Some(reference) = input.source.as_ref() {
                return basecraft_ops::records::verified_source(directory, reference);
            }
        }
        let hash = basecraft_io::hash_file(source)?;
        let project = crate::chosen_project::chosen().map_err(basecraft_ops::Error::Invalid)?;
        let run = basecraft_ops::records::reference_input_with_dependencies(
            anchor,
            source,
            &hash,
            &[],
            "GUI Sanger reference",
            project.as_ref(),
        )?;
        basecraft_ops::records::preserve_input(run.directory())?;
        let input = basecraft_ops::records::read_input(run.directory())?;
        let reference = input
            .source
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Missing reference evidence"))?;
        basecraft_ops::records::verified_source(run.directory(), reference)
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
    /// The file the stage reads, for the job list.
    fn source(&self) -> String {
        match self {
            Self::Prepare(reads, _) => reads.first(),
            Self::Orient(path) | Self::Place(path) => Some(path),
            Self::Single(_) | Self::Primers(_) => None,
        }
        .map(|path| path.display().to_string())
        .unwrap_or_default()
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
    pub session: SangerReview,
    pub view: SangerReviewView,
    pub path: PathBuf,
    pub payload: String,
}
/// A running stage. Dropping it cancels the work; a replaced stage's result
/// is ignored by its id.
impl crate::jobs::Outcome for Completion {
    fn ended(&self) -> crate::jobs::Ended {
        crate::jobs::Ended {
            error: None,
            result: self.path.display().to_string(),
        }
    }
}
pub(super) struct Stage {
    pub id: u64,
    pub name: &'static str,
    pub work: basecraft_ops::work::Work,
}
impl Drop for Stage {
    fn drop(&mut self) {
        self.work.cancel();
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
    let registry = crate::plugins::registry().map_err(|e| e.to_string())?;
    let mut request = serde_json::json!({"operation": operation});
    request["inputs"] = inputs;
    request["parameters"] = parameters;
    let request = serde_json::from_value(request).map_err(|e| e.to_string())?;
    let policy = SessionPolicy::new(root, true).map_err(|e| e.to_string())?;
    let accepted = jobs
        .submit(&registry, request, policy, "GUI")
        .map_err(|e| e.to_string())?;
    // The shared wait drives the run; a detached session owns no document.
    let mut session = basecraft_ops::Session::detached(root);
    let outcome = jobs
        .wait_in_session(&accepted.run_id, &mut session, &|| {
            cancelled.load(Ordering::Acquire)
        })
        .map_err(|e| e.to_string())?;
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
    Ok(outcome)
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
fn edit_analysis(review: &mut SangerReview, edit: ReviewOp) -> Result<(), String> {
    let mut owner = ReviewDocument::new(review)?;
    owner.apply(edit)?;
    *review = owner.review().ok_or("No review is open")?.clone();
    Ok(())
}
fn evaluate(session: &mut SangerReview, view: &mut SangerReviewView) -> Result<(), String> {
    view.review_settings = session.settings.clone();
    edit_analysis(
        session,
        ReviewOp::Evaluate {
            version: 2,
            settings: session.settings.clone(),
            calls: (0..view.draft.columns.len())
                .map(|i| view.assessment(i).call)
                .collect(),
            reference: view.frozen_reference(),
        },
    )
}

pub(super) fn start(
    owner: cxx_qt::CxxQtThread<super::qobject::SangerController>,
    request: Request,
    session: Option<SangerReview>,
) -> Result<Stage, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = request.name();
    let listing = crate::jobs::Listing {
        operation: request.message().into(),
        source: request.source(),
    };
    let work = crate::jobs::start_listed(
        owner,
        listing,
        move |work| execute(request, session, work.flag()),
        move |this, result| this.stage_done(id, result),
    )
    .map_err(|e| e.to_string())?;
    Ok(Stage { id, name, work })
}
fn execute(
    request: Request,
    previous: Option<SangerReview>,
    cancelled: &AtomicBool,
) -> Result<Completion, String> {
    check(cancelled)?;
    let mut jobs = JobManager::default().with_task_cancellation();
    let mut payload = String::new();
    let (session, path) = match request {
        Request::Prepare(paths, threshold) => {
            let captures = paths
                .iter()
                .map(|path| {
                    basecraft_ops::sanger_review::CapturedInput::open(path)
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            jobs = jobs.with_captured_inputs(captures.clone());
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
            let mut session = SangerReview::from_draft(SangerDraft::read(&path)?, first)?;
            session.use_captured_inputs(captures)?;
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
                    let started =
                        caller.start_captured(session.captured_inputs()?.remove(0), threshold)?;
                    let called = if let Some(called) = started {
                        called
                    } else {
                        let called = caller.wait(cancelled);
                        check(cancelled)?;
                        called?
                    };
                    let mut alternate = SangerReview::from_draft(called.draft, &source.path)?;
                    alternate.use_captured_inputs(session.captured_inputs()?)?;
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
                        let mut oriented = SangerReview::from_draft(draft, &path)?;
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
                        crate::chosen_project::chosen()?.as_ref(),
                    )?;
                    let mut view = session_view(&session)?;
                    let automatic = session.automatic_only();
                    evaluate(&mut session, &mut view)?;
                    if automatic && let Some((start, end)) = view.reference_mask_bounds() {
                        let mut settings = session.mask.clone();
                        settings.start = start;
                        settings.end = end;
                        edit_analysis(&mut session, ReviewOp::Mask { settings })?;
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
    check(cancelled)?;
    Ok(Completion {
        session,
        view,
        path,
        payload,
    })
}

/// The editor owns a regular Session and its checked document handle.
/// This adapter holds no history, checkpoint or save implementation.
/// Why a review could not start. A folder that cannot hold recovery stays
/// typed, so the window can offer a copy (D13).
#[derive(Debug)]
pub(super) enum ReviewFailure {
    ReadOnlyFolder,
    Failed(String),
}
impl ReviewFailure {
    pub fn read_only(&self) -> bool {
        matches!(self, Self::ReadOnlyFolder)
    }
}
impl From<ReviewFailure> for String {
    fn from(failure: ReviewFailure) -> Self {
        match failure {
            ReviewFailure::ReadOnlyFolder => crate::document_open::READ_ONLY_EDIT.into(),
            ReviewFailure::Failed(message) => message,
        }
    }
}

pub(super) struct ReviewDocument {
    pub session: basecraft_ops::Session,
    pub id: basecraft_ops::DocId,
}
impl ReviewDocument {
    pub fn new(review: &SangerReview) -> Result<Self, ReviewFailure> {
        let mut session = crate::chosen_project::session_for(review.storage_anchor())
            .map_err(ReviewFailure::Failed)?;
        let id = session.create_sanger_review(review).map_err(|e| match e {
            basecraft_ops::Error::ReadOnlyFolder { .. } => ReviewFailure::ReadOnlyFolder,
            e => ReviewFailure::Failed(e.to_string()),
        })?;
        Ok(Self { session, id })
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut session = crate::chosen_project::session_for(path)?;
        let id = session.open(path).map_err(|e| e.to_string())?;
        session.sanger_review(id).map_err(|e| e.to_string())?;
        Ok(Self { session, id })
    }
    pub fn report(&self, _dirty: bool) -> Result<basecraft_ops::records::Report, String> {
        self.review()
            .ok_or("No review is open")?
            .report_with_history(
                &self
                    .session
                    .document_record(self.id)
                    .map_err(|e| e.to_string())?,
            )
    }
    pub fn review(&self) -> Option<&SangerReview> {
        self.session.sanger_review(self.id).ok()
    }
    /// A review decision, as the CLI's `sanger decide` and MCP make it.
    pub fn apply(&mut self, edit: ReviewOp) -> Result<(), String> {
        basecraft_ops::sanger_commands::decide(&mut self.session, self.id, edit)
            .map_err(|e| crate::document_open::edit_failure(&e))
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.session
            .save_new(self.id, Some(path))
            .map_err(|e| e.to_string())
    }
    pub fn update(&mut self, review: &SangerReview) -> Result<(), String> {
        self.session
            .update_sanger_review(self.id, review)
            .map_err(|e| e.to_string())
    }
    pub fn select_call_set(&mut self, id: &str) -> Result<(), String> {
        self.session
            .apply(
                self.id,
                basecraft_protocol::Op::SangerSelect {
                    call_set: id.into(),
                },
            )
            .map(|_| ())
            .map_err(|e| crate::document_open::edit_failure(&e))
    }
    pub fn export_consensus(&mut self, path: &Path, label: &str) -> Result<(), String> {
        self.session
            .export_sanger_consensus(self.id, path, label)
            .map_err(|e| e.to_string())
    }
    pub fn addition_evidence(&mut self) -> Result<String, String> {
        self.review()
            .ok_or("No review is open")?
            .addition_evidence(crate::chosen_project::chosen()?.as_ref())
    }
}

pub(super) fn load_draft_review(path: &Path) -> Result<(SangerReviewView, SangerReview), String> {
    let mut session = SangerReview::new(path)?;
    // Opening evidence is inspection. Re-running Tracy to enrich older
    // evidence must be an explicit analysis, never a passive-open write.
    let mut view = session_view(&session)?;
    let calls = (0..view.draft.columns.len())
        .map(|i| view.assessment(i).call)
        .collect();
    edit_analysis(
        &mut session,
        ReviewOp::Evaluate {
            version: 2,
            settings: view.review_settings.clone(),
            calls,
            reference: None,
        },
    )?;
    if let Some(i) = (0..view.draft.columns.len()).find(|&i| view.requires_review(i)) {
        view.select_column(i);
    }
    Ok((view, session))
}

pub(super) fn sync_review_view(review: &mut SangerReviewView, session: &SangerReview) {
    review.review_settings.clone_from(&session.settings);
    review.reference_evidence.clone_from(&session.reference);
    for i in 0..review.consensus.len() {
        review.consensus[i] = match session.decision(i) {
            Some(ReviewChoice::Edit(base) | ReviewChoice::Auto(base)) => *base as u8,
            _ => review.draft.columns[i].consensus as u8,
        };
        review.masked[i] = session.masked(i);
        review.unresolved[i] = !session.masked(i)
            && matches!(
                session.decision(i),
                Some(ReviewChoice::Pending | ReviewChoice::Reject)
            );
    }
}

pub(super) fn session_view(session: &SangerReview) -> Result<SangerReviewView, String> {
    let draft = session.working_draft()?;
    let traces = session
        .captured_inputs()?
        .iter()
        .map(|capture| abif::parse(capture.bytes()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut view = SangerReviewView::new(draft, traces)?;
    if let Some(placement) = session.placement()? {
        let indexed = Arc::new(
            basecraft_io::fasta::IndexedFasta::open(&placement.source_path)
                .map_err(|e| e.to_string())?,
        );
        let alignment =
            AlignmentDoc::from_fasta(placement.source_path.display().to_string(), &indexed);
        view.attach_alignment(placement, alignment)?;
    }
    Ok(view)
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
        let request = Request::Prepare(reads.clone(), 20);
        let preparation =
            std::thread::spawn(move || execute(request, None, &AtomicBool::new(false)));
        // Dispose the alignment-owned snapshot before observing first completion.
        std::fs::remove_file(&reference).unwrap();
        assert_eq!(basecraft_io::hash_file(&frozen).unwrap(), original_hash);
        assert_eq!(retain_reference(&reads[0], &frozen).unwrap(), frozen);
        let prepared = preparation
            .join()
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
        let placed = execute(
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
        ReviewDocument::new(&placed.session)
            .unwrap()
            .save(&saved)
            .unwrap();
        let reopened = SangerReview::load(&saved).unwrap();
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
