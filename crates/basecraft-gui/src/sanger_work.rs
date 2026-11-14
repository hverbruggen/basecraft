//! Sanger continuations run outside Qt. Scientific execution and receipts belong
//! exclusively to the registry/JobManager; this module only adapts review state.
use super::{Path, PathBuf, ReviewChoice, ReviewOp, SangerReview, SangerReviewView};
use basecraft_docs::{
    AlignmentDoc,
    sanger::{SangerDraft, SangerDraftExt},
};
use basecraft_io::abif;
use basecraft_ops::Error as OpsError;
use basecraft_plugin_host::{Error, Result, jobs::JobManager, policy::SessionPolicy};
use basecraft_protocol::operation::{OperationErrorCode as Code, OperationOutcome, RuntimeState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The project chosen for this window; its folder may have gone missing.
fn chosen() -> basecraft_ops::Result<Option<basecraft_ops::project::Project>> {
    crate::chosen_project::chosen().map_err(OpsError::NotReady)
}

/// What a review needs before this step can run; the reason is for people.
fn not_ready(reason: &str) -> Error {
    OpsError::NotReady(reason.into()).into()
}

/// Explicit request evidence uses the shared preserve-copy ledger. The returned
/// verified path outlives source-controller snapshots, failed stages and retries.
/// Capture occurs before dispatch: a worker must never race snapshot disposal.
pub(super) fn retain_reference(anchor: &Path, source: &Path) -> basecraft_ops::Result<PathBuf> {
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
        let project = chosen()?;
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
/// Why a cancelled stage ended; the review it would have replaced is kept.
pub(super) const CANCELLED: &str = "Sanger operation cancelled; previous review preserved";

fn check(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        Err(OpsError::Cancelled(CANCELLED.into()).into())
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
) -> Result<OperationOutcome> {
    check(cancelled)?;
    let registry = crate::plugins::registry()?;
    let mut request = serde_json::json!({"operation": operation});
    request["inputs"] = inputs;
    request["parameters"] = parameters;
    let request = serde_json::from_value(request)?;
    let policy = SessionPolicy::new(root, true)?;
    let accepted = jobs.submit(&registry, request, policy, "GUI")?;
    // The shared wait drives the run; a detached session owns no document.
    let mut session = basecraft_ops::Session::detached(root);
    let outcome = jobs.wait_in_session(&accepted.run_id, &mut session, &|| {
        cancelled.load(Ordering::Acquire)
    })?;
    check(cancelled)?;
    if outcome.state != RuntimeState::Completed {
        let code = outcome.errors.first().map_or(Code::ToolFailure, |e| e.code);
        return Err(Error::refused(
            code,
            format!(
                "{:?}: {}. Retained evidence: {}",
                outcome.state,
                outcome
                    .errors
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
                outcome.evidence.as_deref().unwrap_or("unavailable")
            ),
        ));
    }
    Ok(outcome)
}
fn artifact(outcome: &OperationOutcome, name: &str) -> Result<PathBuf> {
    outcome
        .artifacts
        .iter()
        .find(|a| Path::new(&a.path).file_name().is_some_and(|n| n == name))
        .map(|a| PathBuf::from(&a.path))
        .ok_or_else(|| {
            Error::refused(
                Code::PluginProtocolViolation,
                format!("Operation did not retain {name}"),
            )
        })
}
fn directory(outcome: &OperationOutcome) -> Result<&Path> {
    outcome.evidence.as_deref().map(Path::new).ok_or_else(|| {
        Error::refused(
            Code::EvidenceIncomplete,
            "Operation receipt has no evidence directory".into(),
        )
    })
}
fn edit_analysis(review: &mut SangerReview, edit: ReviewOp) -> basecraft_ops::Result<()> {
    let mut owner = ReviewDocument::new(review)?;
    owner.apply(edit)?;
    *review = owner.session.sanger_review(owner.id)?.clone();
    Ok(())
}
fn evaluate(session: &mut SangerReview, view: &mut SangerReviewView) -> basecraft_ops::Result<()> {
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
) -> std::io::Result<Stage> {
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
    )?;
    Ok(Stage { id, name, work })
}
fn execute(
    request: Request,
    previous: Option<SangerReview>,
    cancelled: &AtomicBool,
) -> Result<Completion> {
    check(cancelled)?;
    let mut jobs = JobManager::default();
    let mut payload = String::new();
    let (session, path) = match request {
        Request::Prepare(paths, threshold) => {
            let captures = paths
                .iter()
                .map(|path| basecraft_ops::sanger_review::CapturedInput::open(path))
                .collect::<basecraft_ops::Result<Vec<_>>>()?;
            jobs = jobs.with_captured_inputs(captures.clone());
            let first = paths
                .first()
                .ok_or_else(|| OpsError::MissingChoice("Choose two or three AB1 reads".into()))?;
            let root = first
                .parent()
                .ok_or_else(|| OpsError::NoFileName(first.clone()))?;
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
            let draft = basecraft_ops::sanger_review::read_evidence(&path, SangerDraft::validate)?;
            let mut session = SangerReview::from_draft(draft, first)?;
            session.use_captured_inputs(captures)?;
            let mut view = session_view(&session)?;
            evaluate(&mut session, &mut view)?;
            session.attach_operation_run(directory(&outcome)?)?;
            (session, path)
        }
        request => {
            let mut session = previous.ok_or_else(|| not_ready("Open a Sanger review first"))?;
            let path = session.storage_anchor().to_path_buf();
            let root = path
                .parent()
                .ok_or_else(|| OpsError::NoFileName(path.clone()))?;
            match request {
                Request::Single(threshold) => {
                    let draft = session.working_draft()?;
                    if draft.inputs.len() != 1 {
                        return Err(OpsError::MissingChoice(
                            "Choose one trace to compute calls".into(),
                        )
                        .into());
                    }
                    let source = &draft.inputs[0];
                    let mut caller = basecraft_plugin_host::sanger_calls::SangerCaller::default();
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
                    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(
                        artifact(&outcome, "addition.json")?,
                    )?)?;
                    if value["reversed"].as_array().is_some_and(|a| !a.is_empty()) {
                        if !session.automatic_only() {
                            return Err(not_ready(
                                "This review already has decisions; open a fresh draft to change orientation",
                            ));
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
                        chosen()?.as_ref(),
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
                    payload = std::fs::read_to_string(artifact(&outcome, "primers.json")?)?;
                    session.attach_operation_run(directory(&outcome)?)?;
                }
                Request::Prepare(..) => {
                    return Err(
                        OpsError::Invalid("Unexpected nested preparation request".into()).into(),
                    );
                }
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
pub(super) struct ReviewDocument {
    pub session: basecraft_ops::Session,
    pub id: basecraft_ops::DocId,
}

/// A recoverable session for `file`; the chosen project's folder may be missing.
fn session_for(file: &Path) -> basecraft_ops::Result<basecraft_ops::Session> {
    crate::chosen_project::session_for(file).map_err(OpsError::NotReady)
}

impl ReviewDocument {
    /// A folder that cannot hold recovery stays typed (`ReadOnlyFolder`), so
    /// the window can offer a copy (D13).
    pub fn new(review: &SangerReview) -> basecraft_ops::Result<Self> {
        let mut session = session_for(review.storage_anchor())?;
        let id = session.create_sanger_review(review)?;
        Ok(Self { session, id })
    }
    pub fn open(path: &Path) -> basecraft_ops::Result<Self> {
        let mut session = session_for(path)?;
        let id = session.open(path)?;
        session.sanger_review(id)?;
        Ok(Self { session, id })
    }
    pub fn report(&self, _dirty: bool) -> basecraft_ops::Result<basecraft_ops::records::Report> {
        self.session
            .sanger_review(self.id)?
            .report_with_history(&self.session.document_record(self.id)?)
    }
    pub fn review(&self) -> Option<&SangerReview> {
        self.session.sanger_review(self.id).ok()
    }
    /// A review decision, as the CLI's `sanger decide` and MCP make it.
    pub fn apply(&mut self, edit: ReviewOp) -> basecraft_ops::Result<()> {
        basecraft_ops::sanger_commands::decide(&mut self.session, self.id, edit)
    }

    pub fn save(&mut self, path: &Path) -> basecraft_ops::Result<()> {
        self.session.save_new(self.id, Some(path))
    }
    pub fn update(&mut self, review: &SangerReview) -> basecraft_ops::Result<()> {
        self.session.update_sanger_review(self.id, review)
    }
    pub fn select_call_set(&mut self, id: &str) -> basecraft_ops::Result<()> {
        self.session
            .apply(
                self.id,
                basecraft_protocol::Op::SangerSelect {
                    call_set: id.into(),
                },
            )
            .map(|_| ())
    }
    pub fn export_consensus(&mut self, path: &Path, label: &str) -> basecraft_ops::Result<()> {
        self.session.export_sanger_consensus(self.id, path, label)
    }
    pub fn addition_evidence(&mut self) -> basecraft_ops::Result<String> {
        let project = chosen()?;
        self.session
            .sanger_review(self.id)?
            .addition_evidence(project.as_ref())
    }
}

pub(super) fn load_draft_review(
    path: &Path,
) -> basecraft_ops::Result<(SangerReviewView, SangerReview)> {
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

pub(super) fn session_view(session: &SangerReview) -> basecraft_ops::Result<SangerReviewView> {
    let draft = session.working_draft()?;
    let traces = session
        .captured_inputs()?
        .iter()
        .map(|capture| abif::parse(capture.bytes()))
        .collect::<basecraft_io::Result<Vec<_>>>()?;
    let mut view = SangerReviewView::new(draft, traces).map_err(OpsError::Invalid)?;
    if let Some(placement) = session.placement()? {
        let source = placement.source_path.clone();
        let indexed = Arc::new(basecraft_io::fasta::IndexedFasta::open(&source)?);
        let alignment = AlignmentDoc::from_fasta(source.display().to_string(), &indexed);
        view.attach_alignment(placement, alignment)
            .map_err(|detail| OpsError::Damaged {
                path: source,
                detail,
            })?;
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
        assert_eq!(result.err().unwrap().to_string(), CANCELLED);
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
