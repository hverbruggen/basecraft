//! Sanger adapters for the common artifact store and one-source evidence ledgers.
use super::{SangerReview, StoredReview};
use crate::{
    error::invalid, records::analysis_store::ArtifactRun, records::input_evidence as inputs,
};
use basecraft_docs::sanger::SangerPlacementExt;
use basecraft_docs::sanger::{SangerDraft, SangerPlacement};
use basecraft_protocol::operation::RuntimeState;
use basecraft_protocol::provenance::{ArtifactRevision, Completeness, Publication};
use basecraft_protocol::sanger_records::SangerRequest;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) use basecraft_protocol::sanger_records::{
    ReviewEvidence, ReviewSource as Source, ReviewSupport as Support,
};

pub(super) trait EvidencePaths {
    fn map_paths(&mut self, map: impl FnMut(&Path) -> crate::Result<PathBuf>) -> crate::Result<()>;
}

impl EvidencePaths for ReviewEvidence {
    fn map_paths(
        &mut self,
        mut map: impl FnMut(&Path) -> crate::Result<PathBuf>,
    ) -> crate::Result<()> {
        for run in &mut self.runs {
            *run = map(run)?;
        }
        for source in &mut self.sources {
            source.path = map(&source.path)?;
            if let Some(ledger) = &mut source.ledger {
                *ledger = map(ledger)?;
            }
        }
        if let Some(placement) = &mut self.placement {
            placement.path = map(&placement.path)?;
        }
        Ok(())
    }
}

fn base(path: &Path) -> crate::Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match parent.canonicalize() {
        Ok(parent) => Ok(parent),
        // Recovery encodes for its final directory before atomic publication.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalized(&std::path::absolute(parent)?))
        }
        Err(error) => Err(error.into()),
    }
}

// Placement contexts can be large alignments; only traces use owned captures.
// A context inside the project links the history it keeps there. It is read
// once, for both hashes; with `sha256`, it must still hold those bytes.
fn source(
    path: &Path,
    sha256: Option<&str>,
    anchor: &Path,
    project: Option<&crate::project::Project>,
) -> crate::Result<Source> {
    let path = path.canonicalize()?;
    let stamp = inputs::stamp(&path)?;
    let (hash, actual) = basecraft_io::hash_file_with_sha256(&path)?;
    if inputs::stamp(&path)? != stamp
        || sha256.is_some_and(|sha256| !actual.eq_ignore_ascii_case(sha256))
    {
        return Err(crate::Error::ChangedOnDisk { path });
    }
    let ledger = inputs::Mutation::begin_in(project)?.reference_input_with_dependencies(
        anchor,
        &path,
        &hash,
        Some(&stamp),
        &[],
        "Explicit Sanger input reference",
        project,
    )?;
    let reference = inputs::read_input(ledger.directory())?.source;
    Ok(Source {
        original: path.clone(),
        path,
        sha256: sha256.map_or(actual, Into::into),
        ledger: Some(ledger.directory().into()),
        reference,
    })
}

fn source_capture(
    capture: &crate::sanger_review::CapturedInput,
    anchor: &Path,
    project: Option<&crate::project::Project>,
) -> crate::Result<Source> {
    let ledger = inputs::Mutation::begin_in(project)?.reference_input_with_dependencies(
        anchor,
        capture.path(),
        capture.hash(),
        Some(capture.stamp()),
        &[],
        "Explicit Sanger input reference",
        project,
    )?;
    let reference = inputs::read_input(ledger.directory())?.source;
    Ok(Source {
        original: capture.path().into(),
        path: capture.path().into(),
        sha256: capture.sha256().into(),
        ledger: Some(ledger.directory().into()),
        reference,
    })
}

pub(super) trait SourceAccess {
    fn location(&self) -> crate::Result<PathBuf>;
    fn capture(&self) -> crate::Result<crate::sanger_review::CapturedInput>;
    fn checked(&self) -> crate::Result<PathBuf>;
}

impl SourceAccess for Source {
    fn location(&self) -> crate::Result<PathBuf> {
        let path = match (&self.ledger, &self.reference) {
            (Some(ledger), Some(reference)) => {
                let path = inputs::source_location(ledger, reference)?;
                if !path.try_exists()?
                    && inputs::read_input(ledger)?.state
                        == basecraft_protocol::input_records::EvidenceState::Referenced
                {
                    self.path.clone()
                } else {
                    path
                }
            }
            (None, None) => self.path.clone(),
            _ => {
                return Err(crate::Error::Damaged {
                    path: self.path.clone(),
                    detail: "Incomplete Sanger input reference".into(),
                });
            }
        };
        Ok(path)
    }
    fn checked(&self) -> crate::Result<PathBuf> {
        let path = self.location()?;
        if !basecraft_io::sha256_file(&path)?.eq_ignore_ascii_case(&self.sha256)
            || self
                .reference
                .as_ref()
                .map(|r| basecraft_io::hash_file(&path).map(|hash| hash != r.hash))
                .transpose()?
                .unwrap_or(false)
        {
            return Err(crate::Error::ChangedOnDisk { path });
        }
        Ok(path)
    }
    fn capture(&self) -> crate::Result<crate::sanger_review::CapturedInput> {
        let path = self.location()?;
        let capture = crate::sanger_review::CapturedInput::open(&path)?;
        if !capture.sha256().eq_ignore_ascii_case(&self.sha256)
            || self.reference.as_ref().is_some_and(|r| {
                r.hash != capture.hash() || r.stamp.size != capture.bytes().len() as u64
            })
        {
            return Err(crate::Error::ChangedOnDisk { path });
        }
        Ok(capture)
    }
}

/// An explicit Sanger analysis reserves durable support before invoking a tool.
/// Errors/cancellation never remove its directory or an original input.
pub struct SangerRun {
    captures: Vec<crate::sanger_review::CapturedInput>,
    run: ArtifactRun,
    evidence: ReviewEvidence,
    operation: String,
    anchor: PathBuf,
    /// What the stage's run link is relative to: the project root, or the
    /// anchor's folder outside a project.
    base: PathBuf,
    terminal: std::cell::Cell<bool>,
    started: String,
}

fn linked_input(run: &ArtifactRun, dependencies: &[PathBuf], payload: &[u8]) -> crate::Result<()> {
    inputs::link_generated_input(
        run,
        dependencies,
        payload,
        "identity-v1",
        "Sanger review/support",
    )
}

fn retained(run: &ArtifactRun, path: &Path, index: usize) -> crate::Result<ArtifactRevision> {
    let name = path
        .strip_prefix(run.directory())
        .map_err(|_| invalid("Sanger support escapes its run"))?;
    if name
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(invalid("Unsafe Sanger support path"));
    }
    if !path
        .canonicalize()?
        .starts_with(run.directory().canonicalize()?)
    {
        return Err(invalid("Sanger support escapes its owned directory"));
    }
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid("Sanger support is a symlink"));
    }
    basecraft_platform::files::sync_file(path)?;
    Ok(ArtifactRevision {
        id: format!("{}:support:{index}", run.id()),
        logical_name: name.to_string_lossy().into_owned(),
        hash: basecraft_io::hash_file(path)?,
        size: std::fs::metadata(path)?.len(),
        retained: Some(name.to_string_lossy().into_owned()),
        reference: None,
    })
}

fn publication(run: &ArtifactRun, origin: &str, artifacts: Vec<ArtifactRevision>) -> Publication {
    Publication {
        schema_version: 1,
        id: run.id().into(),
        session: run.id().into(),
        parents: vec![],
        parent_hashes: BTreeMap::new(),
        created: basecraft_platform::now_rfc3339(),
        origin: origin.into(),
        inputs: artifacts,
        outputs: vec![],
        events: vec![],
        effective_edits: vec![],
        withdrawn: Vec::new(),
        completeness: Completeness {
            ancestry_known: false,
            input_bytes_available: true,
            execution_specification_available: false,
            replay_verified: false,
        },
        report_template_version: 2,
    }
}

impl SangerRun {
    pub fn prepare(
        anchor: &Path,
        operation: &str,
        paths: &[PathBuf],
        project: Option<&crate::project::Project>,
    ) -> crate::Result<Self> {
        Self::prepare_with_dependencies(anchor, operation, paths, &[], project)
    }

    pub fn prepare_with_dependencies(
        anchor: &Path,
        operation: &str,
        paths: &[PathBuf],
        ancestors: &[PathBuf],
        project: Option<&crate::project::Project>,
    ) -> crate::Result<Self> {
        if paths.is_empty() {
            return Err(crate::Error::Unsupported(
                "Sanger requires at least one input file".into(),
            ));
        }
        if paths.len() > 4 {
            return Err(crate::Error::Limit {
                what: "Sanger input files".into(),
                limit: 4,
            });
        }
        let mut evidence = ReviewEvidence::default();
        let mut captures = Vec::new();
        for path in paths {
            if path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("ab1") || ext.eq_ignore_ascii_case("abi")
            }) {
                let capture = crate::sanger_review::CapturedInput::open(path)?;
                evidence
                    .sources
                    .push(source_capture(&capture, anchor, project)?);
                captures.push(capture);
            } else {
                evidence.sources.push(source(path, None, anchor, project)?);
            }
        }
        let run = ArtifactRun::create(project, anchor, &ulid::Ulid::generate().to_string())?;
        let mut dependencies: Vec<_> = evidence
            .sources
            .iter()
            .filter_map(|s| s.ledger.clone())
            .collect();
        dependencies.extend_from_slice(ancestors);
        // Sources relative to the run folder, so the request moves with it.
        let sources = paths
            .iter()
            .map(|path| {
                inputs::relative(run.directory(), &std::path::absolute(path)?).map(PathBuf::from)
            })
            .collect::<crate::Result<_>>()?;
        let payload = serde_json::to_vec(&serde_json::to_value(SangerRequest {
            operation: operation.into(),
            sources,
        })?)?;
        inputs::write_json(
            &run.artifact("request.json")?,
            &serde_json::from_slice::<serde_json::Value>(&payload)?,
        )?;
        linked_input(&run, &dependencies, &payload)?;
        let result = Self {
            captures,
            run,
            evidence,
            operation: operation.into(),
            anchor: std::path::absolute(anchor)?,
            base: crate::project::Project::base_of(project, anchor)?,
            terminal: std::cell::Cell::new(false),
            started: basecraft_platform::now_rfc3339(),
        };
        result.write_run_record()?;
        result.status(RuntimeState::Running, None)?;
        Ok(result)
    }

    pub fn captured_inputs(&self) -> &[crate::sanger_review::CapturedInput] {
        &self.captures
    }

    pub fn review_snapshot(&self, session: &SangerReview) -> crate::Result<PathBuf> {
        let path = self.artifact("source-review.sanger-review.json")?;
        session.write_embedded(&path)?;
        Ok(path)
    }

    pub fn directory(&self) -> &Path {
        self.run.directory()
    }
    pub fn artifact(&self, name: &str) -> crate::Result<PathBuf> {
        self.run.artifact(name)
    }

    /// The stage's run record, as every tool run has one: its sources with
    /// paths relative to the run folder and their hashes. A stage records no
    /// settings yet, so `effective_parameters` is null and methods say so.
    fn write_run_record(&self) -> crate::Result<()> {
        let folder = self.run.directory();
        let sources = self
            .evidence
            .sources
            .iter()
            .map(|source| {
                let hash = format!("sha256:{}", source.sha256);
                Ok(basecraft_protocol::operation::InvocationInputEvidence {
                    original_path: Some(inputs::relative(folder, &source.location()?)?),
                    source_hash: Some(hash.clone()),
                    submitted_hash: hash,
                    transformation: "identity-v1".into(),
                    run_directory: match &source.ledger {
                        Some(ledger) => inputs::relative(folder, ledger)?,
                        None => ".".into(),
                    },
                })
            })
            .collect::<crate::Result<_>>()?;
        let record = basecraft_protocol::records::RunRecord {
            initiator: "basecraft".into(),
            process_instance: basecraft_platform::process_instance().into(),
            inputs: std::collections::BTreeMap::from([("sources".into(), sources)]),
            ..basecraft_protocol::records::RunRecord::new(
                format!("{SANGER_TOOL}/{}", self.operation),
                basecraft_protocol::VERSION.into(),
                SANGER_TOOL.into(),
            )
        };
        inputs::write_json(&self.run.artifact("execution.json")?, &record)
    }

    /// Write the stage's status as its run's `runtime.json`, the record
    /// the Tasks view lists (D28).
    fn status(&self, state: RuntimeState, diagnostic: Option<&str>) -> crate::Result<()> {
        let operation = self.operation.clone();
        let errors = diagnostic.map(|message| basecraft_protocol::operation::OperationError {
            code: basecraft_protocol::operation::OperationErrorCode::ToolFailure,
            message: message.into(),
            field: None,
            operation: operation.clone(),
            run_id: Some(self.run.id().into()),
            evidence: None,
        });
        inputs::write_json(
            &self.run.artifact("runtime.json")?,
            &basecraft_protocol::operation::OperationOutcome {
                schema: 1,
                operation,
                run_id: self.run.id().into(),
                state,
                pins: basecraft_protocol::operation::OperationPins {
                    generation: String::new(),
                    package_digest: String::new(),
                    descriptor_digest: String::new(),
                },
                requested_parameters: serde_json::Value::Null,
                effective_parameters: serde_json::Value::Null,
                data: None,
                artifacts: Vec::new(),
                proposals: None,
                warnings: Vec::new(),
                errors: errors.into_iter().collect(),
                evidence: Some(self.run.directory().display().to_string()),
            },
        )
    }

    pub fn failed(&self, diagnostic: &str) -> crate::Result<()> {
        if self.terminal.get() {
            return Err(invalid("Sanger run already terminated"));
        }
        self.status(RuntimeState::Failed, Some(diagnostic))?;
        self.terminal.set(true);
        Ok(())
    }

    pub fn cancelled(&self, diagnostic: &str) -> crate::Result<()> {
        if self.terminal.get() {
            return Err(invalid("Sanger run already terminated"));
        }
        self.status(RuntimeState::Cancelled, Some(diagnostic))?;
        self.terminal.set(true);
        Ok(())
    }

    /// Retain only declared, checked output files. The invocation directory itself
    /// stays durable even if a plugin fails before returning its declaration.
    pub fn finish(mut self, paths: &[PathBuf]) -> crate::Result<Self> {
        if self.terminal.get() || paths.is_empty() {
            return Err(invalid("Sanger run terminated or has no declared outputs"));
        }
        let mut artifacts = vec![retained(&self.run, &self.run.artifact("request.json")?, 0)?];
        for (index, path) in paths.iter().enumerate() {
            artifacts.push(retained(&self.run, path, index + 1)?);
        }
        for source in &self.evidence.sources {
            let location = source.location()?;
            if let Some(capture) = self
                .captures
                .iter()
                .find(|capture| capture.path() == source.path)
            {
                if location.canonicalize()? != capture.path() {
                    return Err(crate::Error::ChangedOnDisk {
                        path: source.path.clone(),
                    });
                }
                capture.check_current()?;
            } else {
                source.checked()?;
            }
        }
        let mut publication = publication(
            &self.run,
            &format!("Sanger {} support; not accepted consensus", self.operation),
            artifacts,
        );
        let inputs: Vec<_> = publication
            .inputs
            .iter()
            .map(|input| (input.logical_name.clone(), input.hash.clone()))
            .collect();
        let record = crate::session::ToolRunRecord {
            run: Some(&self.run),
            ..crate::session::ToolRunRecord::new(
                (SANGER_TOOL, &self.operation),
                serde_json::Value::Object(serde_json::Map::new()),
                &inputs,
                &[],
            )
        };
        let entry = crate::records::provenance::file_run_entry(
            record,
            &self.base,
            &publication,
            &self.started,
            sanger_actor(),
            &self.operation,
        )?;
        publication.events.push(entry);
        crate::records::provenance::publish(self.directory(), &publication)?;
        self.evidence.runs.push(self.directory().into());
        self.status(RuntimeState::Completed, None)?;
        self.terminal.set(true);
        Ok(self)
    }
}

impl Drop for SangerRun {
    fn drop(&mut self) {
        if !self.terminal.get() {
            // Best effort during unwinding/early validation failure. The durable
            // reservation and pre-execution record survive even if this write fails.
            let _ = self.status(
                RuntimeState::Interrupted,
                Some("Sanger support was not published; owned files retained"),
            );
        }
    }
}

fn unavailable_sources(evidence: &ReviewEvidence, record: &mut Publication) {
    record.completeness.input_bytes_available =
        evidence.sources.iter().all(|s| s.checked().is_ok());
    for (index, source) in evidence
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| s.ledger.is_none() && s.checked().is_err())
    {
        // SHA-256 remains explicitly identified. No BLAKE3 identity or original
        // byte length may be invented when those bytes are unavailable.
        record.inputs.push(ArtifactRevision {
            id: format!("{}:unavailable:{index}", record.id),
            logical_name: source
                .original
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            hash: format!("sha256:{}", source.sha256),
            size: 0,
            retained: None,
            reference: None,
        });
    }
}

impl SangerReview {
    /// Attach an explicit completed analysis. The exact draft itself is unchanged.
    pub fn attach_run(&mut self, run: &SangerRun) -> Result<(), String> {
        let mut evidence = run.evidence.clone();
        for read in &self.draft.inputs {
            let source = evidence
                .sources
                .iter_mut()
                .find(|s| s.sha256.eq_ignore_ascii_case(&read.sha256))
                .ok_or("Draft input differs from registered Sanger input")?;
            source.original.clone_from(&read.path);
        }
        self.evidence = evidence;
        self.use_captured_inputs(run.captures.clone())?;
        self.anchor.clone_from(&run.anchor);
        Ok(())
    }

    pub fn storage_anchor(&self) -> &Path {
        &self.anchor
    }

    /// Link an already published public-operation run into the review ancestry.
    pub fn attach_operation_run(&mut self, directory: &Path) -> Result<(), String> {
        if !directory.join("manifest.json").is_file() || !directory.join("runtime.json").is_file() {
            return Err("Completed caller support is missing".into());
        }
        if !self.evidence.runs.iter().any(|run| run == directory) {
            self.evidence.runs.push(directory.to_path_buf());
        }
        Ok(())
    }

    /// Prepare a detached analysis result; Session journals it on acceptance.
    #[must_use]
    pub fn fork_for_operation(&self) -> Self {
        self.clone()
    }

    /// Carry already validated review ancestry, for the current call sets.
    /// New analyses still attach through `attach_operation_run`.
    pub fn inherit_review_ancestry(&mut self, previous: &Self) {
        self.evidence.runs.clone_from(&previous.evidence.runs);
        self.evidence.sources.clone_from(&previous.evidence.sources);
    }

    /// Attach placement produced by a registered operation. Its frozen input
    /// must outlive the job's temporary execution directory.
    pub fn attach_operation_placement(
        &mut self,
        directory: &Path,
        path: &Path,
        project: Option<&crate::project::Project>,
    ) -> Result<(), String> {
        let placement = SangerPlacement::read(path)?;
        let context = source(
            &placement.source_path,
            Some(&placement.source_sha256),
            &self.anchor,
            project,
        )
        .map_err(|e| e.to_string())?;
        self.evidence.sources.push(context);
        self.evidence.placement = Some(Support {
            path: path.into(),
            hash: basecraft_io::hash_file(path).map_err(|e| e.to_string())?,
        });
        self.attach_operation_run(directory)
    }

    pub fn evidence_runs(&self) -> &[PathBuf] {
        &self.evidence.runs
    }

    pub(crate) fn share_captures(&mut self, previous: &Self) {
        if let Some(old) = std::iter::once(previous)
            .chain(&previous.retained)
            .find(|old| old.draft_sha256 == self.draft_sha256)
        {
            self.captures = old.captures.clone();
        }
        for set in &mut self.retained {
            set.share_captures(previous);
        }
    }

    pub fn use_captured_inputs(
        &mut self,
        captures: Vec<crate::sanger_review::CapturedInput>,
    ) -> Result<(), String> {
        if captures.len() != self.draft.inputs.len()
            || self
                .draft
                .inputs
                .iter()
                .zip(&captures)
                .any(|(read, c)| !read.sha256.eq_ignore_ascii_case(c.sha256()))
        {
            return Err("Captured traces differ from the caller's evidence".into());
        }
        self.captures = std::sync::Arc::new(std::sync::Mutex::new(
            self.draft
                .inputs
                .iter()
                .zip(captures)
                .map(|(read, capture)| (read.path.clone(), capture))
                .collect(),
        ));
        Ok(())
    }

    /// Immutable trace bytes shared by drawing and tool submission.
    pub fn captured_inputs(&self) -> Result<Vec<crate::sanger_review::CapturedInput>, String> {
        let mut cache = self.captures.lock().map_err(|e| e.to_string())?;
        let mut captures = Vec::new();
        for read in &self.draft.inputs {
            let source =
                self.evidence.sources.iter().find(|s| {
                    s.original == read.path && s.sha256.eq_ignore_ascii_case(&read.sha256)
                });
            let location = source
                .map_or_else(|| Ok(read.path.clone()), SourceAccess::location)
                .and_then(|path| Ok(path.canonicalize()?))
                .map_err(|e: crate::Error| e.to_string())?;
            if let Some(capture) = cache.get(&read.path).filter(|c| c.path() == location) {
                capture.check_current().map_err(|e| e.to_string())?;
                captures.push(capture.clone());
                continue;
            }
            let capture = if let Some(source) = source {
                source.capture()
            } else {
                crate::sanger_review::CapturedInput::open(&read.path)
            }
            .map_err(|e| e.to_string())?;
            if !capture.sha256().eq_ignore_ascii_case(&read.sha256) {
                return Err(format!(
                    "Source AB1 changed since draft: {}",
                    read.path.display()
                ));
            }
            cache.insert(read.path.clone(), capture.clone());
            captures.push(capture);
        }
        Ok(captures)
    }

    /// Resolve live/preserved/packaged paths into a working copy. Immutable draft
    /// JSON and its SHA-256 remain unchanged, including original path spellings.
    pub fn working_draft(&self) -> Result<SangerDraft, String> {
        let mut draft = self.draft.clone();
        for (read, capture) in draft.inputs.iter_mut().zip(self.captured_inputs()?) {
            read.path = capture.path().into();
        }
        Ok(draft)
    }

    pub(crate) fn capture_sources(&mut self) -> crate::Result<()> {
        for set in &mut self.retained {
            set.capture_sources()?;
        }
        for read in &self.draft.inputs {
            if self
                .evidence
                .sources
                .iter()
                .any(|s| s.original == read.path && s.sha256.eq_ignore_ascii_case(&read.sha256))
            {
                continue;
            }
            let candidate = if read.path.is_absolute() {
                read.path.clone()
            } else {
                base(&self.anchor)?.join(&read.path)
            };
            self.evidence.sources.push(Source {
                original: read.path.clone(),
                path: candidate,
                sha256: read.sha256.clone(),
                ledger: None,
                reference: None,
            });
        }
        Ok(())
    }

    pub(crate) fn ensure_sources(&mut self, anchor: &Path) -> crate::Result<()> {
        self.capture_sources()?;
        for set in &mut self.retained {
            set.ensure_sources(anchor)?;
        }
        let captures = self.captured_inputs().unwrap_or_default();
        for input in &mut self.evidence.sources {
            if input.ledger.is_none()
                && let Some(capture) = captures
                    .iter()
                    .find(|c| c.sha256().eq_ignore_ascii_case(&input.sha256))
            {
                // The session's evidence scope carries its project.
                let mut registered = source_capture(capture, anchor, None)?;
                registered.original.clone_from(&input.original);
                *input = registered;
            }
        }
        Ok(())
    }

    pub fn attach_placement(&mut self, run: &SangerRun, path: &Path) -> Result<(), String> {
        let placement = SangerPlacement::read(path)?;
        let context = run
            .evidence
            .sources
            .iter()
            .find(|s| {
                placement_source_matches(&s.original, &placement.source_path)
                    && s.sha256.eq_ignore_ascii_case(&placement.source_sha256)
            })
            .ok_or("Placement source differs from registered alignment")?;
        self.evidence.sources.push(context.clone());
        self.evidence.runs.extend(run.evidence.runs.clone());
        self.evidence.placement = Some(Support {
            path: path.into(),
            hash: basecraft_io::hash_file(path).map_err(|e| e.to_string())?,
        });
        Ok(())
    }

    pub fn placement(&self) -> Result<Option<SangerPlacement>, String> {
        let Some(support) = &self.evidence.placement else {
            return Ok(None);
        };
        if basecraft_io::hash_file(&support.path).map_err(|e| e.to_string())? != support.hash {
            return Err("Retained Sanger placement changed".into());
        }
        let mut placement = SangerPlacement::read(&support.path)?;
        let context = self
            .evidence
            .sources
            .iter()
            .find(|s| {
                placement_source_matches(&s.original, &placement.source_path)
                    && s.sha256.eq_ignore_ascii_case(&placement.source_sha256)
            })
            .ok_or("Placement input reference is missing")?;
        placement.source_path = context.checked().map_err(|e| e.to_string())?;
        Ok(Some(placement))
    }

    fn stored_single_at(&self, path: &Path) -> crate::Result<StoredReview> {
        self.stored_single_with_evidence(path, &self.evidence)
    }

    fn stored_single_with_evidence(
        &self,
        path: &Path,
        evidence: &ReviewEvidence,
    ) -> crate::Result<StoredReview> {
        let mut evidence = evidence.clone();
        let parent = base(path)?;
        evidence.map_paths(|p| Ok(PathBuf::from(inputs::relative(&parent, p)?)))?;
        Ok(StoredReview {
            draft_path: self.draft_path.clone(),
            draft_sha256: self.draft_sha256.clone(),
            draft_json: self.draft_json.clone(),
            events: self.events.clone(),
            evidence,
        })
    }

    fn stored_with_evidence(
        &self,
        path: &Path,
        evidence: &ReviewEvidence,
    ) -> crate::Result<serde_json::Value> {
        let mut snapshot = self.session_snapshot(path)?;
        snapshot.sets[0] = self.stored_single_with_evidence(path, evidence)?;
        Ok(serde_json::to_value(snapshot)?)
    }

    fn write_embedded(&self, path: &Path) -> crate::Result<()> {
        let bytes = self.encoded_at(path, &self.evidence)?;
        basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
        Ok(())
    }

    fn encoded_at(&self, path: &Path, evidence: &ReviewEvidence) -> crate::Result<Vec<u8>> {
        let bytes =
            basecraft_protocol::records::encode(&self.stored_with_evidence(path, evidence)?)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(invalid(
                "Sanger review exceeds 64 MiB; previous file preserved",
            ));
        }
        Ok(bytes)
    }

    pub(crate) fn publish_consensus(
        &mut self,
        project: Option<&crate::project::Project>,
        path: &Path,
    ) -> crate::Result<()> {
        self.ensure_sources(path)?;
        let run = ArtifactRun::create(project, path, &ulid::Ulid::generate().to_string())?;
        let mut dependencies = self.evidence.runs.clone();
        for set in &self.retained {
            dependencies.extend(set.evidence.runs.clone());
            dependencies.extend(
                set.evidence
                    .sources
                    .iter()
                    .filter_map(|source| source.ledger.clone()),
            );
        }
        dependencies.extend(
            self.evidence
                .sources
                .iter()
                .filter_map(|s| s.ledger.clone()),
        );
        dependencies.sort();
        dependencies.dedup();
        linked_input(&run, &dependencies, self.draft_json.as_bytes())?;
        let snapshot = run.artifact("review.sanger-review.json")?;
        self.write_embedded(&snapshot)?;
        let mut record = publication(
            &run,
            "Sanger accepted consensus; full review evidence retained",
            vec![retained(&run, &snapshot, 0)?],
        );
        unavailable_sources(&self.evidence, &mut record);
        let hash = basecraft_io::hash_file(path)?;
        record.outputs.push(ArtifactRevision {
            id: format!("{}:consensus", run.id()),
            logical_name: path
                .file_name()
                .ok_or_else(|| invalid("Consensus has no filename"))?
                .to_string_lossy()
                .into_owned(),
            hash: hash.clone(),
            size: std::fs::metadata(path)?.len(),
            retained: None,
            reference: None,
        });
        crate::records::provenance::publish(run.directory(), &record)?;
        run.bind(project, path, &hash)?;
        self.evidence.runs = vec![run.directory().into()];
        Ok(())
    }
}

/// Package relocation changes only derived location metadata. Original embedded
/// evidence, hashes and all manual/undo/export events remain byte-for-byte values.
pub(crate) fn relocate_package(
    original: &Path,
    copied: &Path,
    bundle: &Path,
    paths: &BTreeMap<PathBuf, String>,
) -> crate::Result<()> {
    let bytes = crate::records::provenance::read_record(copied)?;
    let (mut stored, writer) =
        basecraft_protocol::records::decode_written::<super::StoredCallSets>(&bytes)?;
    if stored.version != 3 {
        return Err(crate::Error::Unsupported(
            basecraft_protocol::records::other_format(&writer, stored.version, 3),
        ));
    }
    for set in &mut stored.sets {
        relocate_stored(set, original, copied, bundle, paths)?;
    }
    let value = serde_json::to_value(stored)?;
    inputs::write_json(copied, &value)
}

fn relocate_stored(
    stored: &mut StoredReview,
    original: &Path,
    copied: &Path,
    bundle: &Path,
    paths: &BTreeMap<PathBuf, String>,
) -> crate::Result<()> {
    let parent = base(original)?;
    stored
        .evidence
        .map_paths(|p| Ok(normalized(&parent.join(p))))?;
    for source in &mut stored.evidence.sources {
        let key = &source.path;
        let hash = paths
            .get(key)
            .ok_or_else(|| invalid("Sanger package is missing a referenced input"))?;
        source.path = PathBuf::from(inputs::relative(
            &base(copied)?,
            &bundle
                .join("artifacts")
                .join(hash.trim_start_matches("b3:")),
        )?);
        source.ledger = None;
        source.reference = None;
    }
    if let Some(placement) = &mut stored.evidence.placement {
        let hash = paths
            .get(&placement.path)
            .ok_or_else(|| invalid("Sanger package is missing placement evidence"))?;
        placement.path = PathBuf::from(inputs::relative(
            &base(copied)?,
            &bundle
                .join("artifacts")
                .join(hash.trim_start_matches("b3:")),
        )?);
    }
    stored.evidence.runs.clear();
    // Every relocated artifact must already have been verified/materialized.
    for source in &stored.evidence.sources {
        let path = copied.parent().unwrap_or(Path::new(".")).join(&source.path);
        if !basecraft_io::sha256_file(&path)?.eq_ignore_ascii_case(&source.sha256) {
            return Err(invalid("Sanger exported source SHA-256 differs"));
        }
    }
    Ok(())
}

fn placement_source_matches(original: &Path, placed: &Path) -> bool {
    if original == placed {
        return true;
    }
    #[cfg(windows)]
    {
        // Windows canonicalization adds verbatim prefixes; equivalent spellings
        // must still resolve to the registered source. Hash checks follow this.
        inputs::relative(original, placed).is_ok_and(|path| path.is_empty())
            || same_file(original, placed).unwrap_or(false)
    }
    #[cfg(not(windows))]
    false
}

fn same_file(a: &Path, b: &Path) -> crate::Result<bool> {
    if normalized(&std::path::absolute(a)?) == normalized(&std::path::absolute(b)?) {
        return Ok(true);
    }
    if let (Ok(a), Ok(b)) = (a.canonicalize(), b.canonicalize())
        && a == b
    {
        return Ok(true);
    }
    #[cfg(unix)]
    if let (Ok(a), Ok(b)) = (std::fs::metadata(a), std::fs::metadata(b)) {
        use std::os::unix::fs::MetadataExt;
        return Ok(a.dev() == b.dev() && a.ino() == b.ino());
    }
    #[cfg(windows)]
    if let (Ok(a), Ok(b)) = (
        basecraft_platform::files::identity(a),
        basecraft_platform::files::identity(b),
    ) {
        return Ok(a == b);
    }
    Ok(false)
}

fn normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub(crate) fn is_review(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        && super::evidence_kind(path) == Some(super::EvidenceKind::Review)
}

#[derive(Serialize, Deserialize)]
struct AdditionEvidence {
    schema_version: u32,
    bundle: PathBuf,
    id: String,
    hash: String,
    review: PathBuf,
    review_hash: String,
    sequence_hash: String,
}

impl SangerReview {
    /// Freeze the accepted input for alignment addition without exporting an AB1
    /// or requiring a visible review file. This descriptor is never executable.
    pub fn addition_evidence(
        &self,
        project: Option<&crate::project::Project>,
    ) -> Result<String, String> {
        let sequence = self
            .resolved_sequence()
            .filter(|s| !s.is_empty())
            .ok_or("Resolve every review column before addition")?;
        let paths: Vec<_> = self
            .working_draft()?
            .inputs
            .into_iter()
            .map(|r| r.path)
            .collect();
        let run = SangerRun::prepare_with_dependencies(
            self.storage_anchor(),
            "accepted_review_input",
            &paths,
            self.evidence_runs(),
            project,
        )
        .map_err(|e| e.to_string())?;
        let review = run.review_snapshot(self).map_err(|e| e.to_string())?;
        let run = run
            .finish(std::slice::from_ref(&review))
            .map_err(|e| e.to_string())?;
        let (_, hash) = crate::records::provenance::latest_record(run.directory())
            .map_err(|e| e.to_string())?;
        let evidence = AdditionEvidence {
            schema_version: 1,
            bundle: run.directory().into(),
            id: run.run.id().into(),
            hash,
            review: review.clone(),
            review_hash: basecraft_io::hash_file(&review).map_err(|e| e.to_string())?,
            sequence_hash: basecraft_io::hash_bytes(sequence.as_bytes()),
        };
        serde_json::to_string(&evidence).map_err(|e| e.to_string())
    }
}

/// An accepted review checked as an alignment input; nothing is recorded yet.
pub struct CheckedAddition(AdditionEvidence);

/// Verify the exact accepted review before selecting it as an alignment
/// input, writing nothing.
pub fn check_addition(descriptor: &str, sequence: &str) -> crate::Result<CheckedAddition> {
    let evidence: AdditionEvidence = serde_json::from_str(descriptor)?;
    if evidence.schema_version != 1
        || evidence.sequence_hash != basecraft_io::hash_bytes(sequence.as_bytes())
    {
        return Err(invalid(
            "Reviewed sequence differs from its Sanger evidence",
        ));
    }
    let (publication, hash) = crate::records::provenance::latest_record(&evidence.bundle)?;
    if publication.id != evidence.id
        || hash != evidence.hash
        || basecraft_io::hash_file(&evidence.review)? != evidence.review_hash
    {
        return Err(invalid("Sanger addition evidence changed"));
    }
    let local = evidence
        .review
        .strip_prefix(&evidence.bundle)
        .map_err(|_| invalid("Sanger review escapes its input run"))?;
    if !publication
        .inputs
        .iter()
        .any(|a| a.retained.as_deref() == local.to_str() && a.hash == evidence.review_hash)
    {
        return Err(invalid(
            "Sanger review is not retained by the selected input run",
        ));
    }
    let review = SangerReview::load(&evidence.review).map_err(invalid)?;
    if review.resolved_sequence().as_deref() != Some(sequence) {
        return Err(invalid(
            "Sanger review is unresolved or describes another sequence",
        ));
    }
    let mut issues = crate::records::provenance::verify_records(&evidence.bundle, &publication)?;
    issues.extend(inputs::verify_ancestors(&evidence.bundle)?);
    if !issues.is_empty() {
        return Err(invalid(issues.join("; ")));
    }
    Ok(CheckedAddition(evidence))
}

/// Verify the accepted review again and record it as the input of the row
/// just added to alignment `doc`. Call it only once the row is in.
pub fn record_addition(
    session: &mut crate::Session,
    doc: crate::DocId,
    descriptor: &str,
    sequence: &str,
) -> crate::Result<()> {
    let CheckedAddition(evidence) = check_addition(descriptor, sequence)?;
    let hash = session.snapshot_file(&evidence.review)?;
    let inputs = [(evidence.review.to_string_lossy().into_owned(), hash)];
    let params = serde_json::to_value(ReviewedConsensus {
        review_hash: &evidence.review_hash,
        sequence_hash: &evidence.sequence_hash,
    })?;
    session.record_tool_run(
        doc,
        sanger_actor(),
        crate::session::ToolRunRecord {
            referenced: true,
            // The run that retains the reviewed consensus.
            input_run: Some(basecraft_protocol::records::ActivityInputRun {
                schema_version: 1,
                id: evidence.id.clone(),
                hash: evidence.hash.clone(),
                extra: std::collections::BTreeMap::new(),
            }),
            ..crate::session::ToolRunRecord::new(
                (SANGER_TOOL, "reviewed_consensus_input"),
                params,
                &inputs,
                &[],
            )
        },
    )
}

const SANGER_TOOL: &str = "basecraft.sanger-review";

fn sanger_actor() -> basecraft_protocol::Actor {
    basecraft_protocol::Actor::Plugin {
        id: SANGER_TOOL.into(),
        version: "1".into(),
    }
}

/// What a reviewed consensus entry records about the review it came from.
#[derive(serde::Serialize)]
struct ReviewedConsensus<'a> {
    review_hash: &'a str,
    sequence_hash: &'a str,
}

impl SangerReview {
    pub(crate) fn session_bytes(&self, target: &Path) -> crate::Result<Vec<u8>> {
        self.encoded_at(target, &self.evidence)
    }

    pub(crate) fn rebase_session_paths(&mut self, from: &Path, to: &Path) {
        let rebase = |path: &Path| {
            Ok(path
                .strip_prefix(from)
                .map_or_else(|_| path.to_owned(), |relative| to.join(relative)))
        };
        let _ = self.evidence.map_paths(rebase);
        if let Ok(relative) = self.anchor.strip_prefix(from) {
            self.anchor = to.join(relative);
        }
        for set in &mut self.retained {
            set.rebase_session_paths(from, to);
        }
    }

    /// Publish the review's support run and stage its bytes; the shared save
    /// owner replaces the target, then `bind_session_review` links the run.
    pub(crate) fn prepare_session_review(
        &self,
        project: Option<&crate::project::Project>,
        target: &Path,
        logical: &Path,
        publication_context: &crate::documents::WritePublication,
    ) -> crate::Result<crate::documents::PreparedWrite> {
        for set in std::iter::once(self).chain(self.retained.iter()) {
            for input in std::iter::once(&set.draft_path)
                .chain(set.draft.inputs.iter().map(|read| &read.path))
                .chain(set.evidence.sources.iter().map(|source| &source.path))
            {
                let input = if input.is_absolute() {
                    input.clone()
                } else {
                    base(&set.anchor)?.join(input)
                };
                if same_file(target, &input)? {
                    return Err(crate::Error::Unsupported(
                        "Review cannot replace a source trace, draft or alignment context".into(),
                    ));
                }
            }
            for ledger in set
                .evidence
                .sources
                .iter()
                .filter_map(|source| source.ledger.as_ref())
            {
                if same_file(target, &ledger.join("preserved-input"))? {
                    return Err(crate::Error::Unsupported(
                        "Review cannot replace preserved source evidence".into(),
                    ));
                }
            }
        }
        let run = ArtifactRun::create(project, target, &ulid::Ulid::generate().to_string())?;
        let mut dependencies: Vec<_> = ArtifactRun::for_result(project, target)?
            .into_iter()
            .collect();
        for set in std::iter::once(self).chain(self.retained.iter()) {
            dependencies.extend(set.evidence.runs.clone());
            dependencies.extend(
                set.evidence
                    .sources
                    .iter()
                    .filter_map(|source| source.ledger.clone()),
            );
        }
        dependencies.sort();
        dependencies.dedup();
        inputs::link_generated_input_locked(
            &run,
            &dependencies,
            self.draft_json.as_bytes(),
            "identity-v1",
            "Sanger review/support",
            |run| publication_context.register(run),
        )?;
        let bytes = self.session_bytes(logical)?;
        // Support must be complete even when public history is disabled or this
        // is an unlogged recovery write. Session may add its history afterward.
        let mut support = publication(&run, "Saved Sanger review support", vec![]);
        unavailable_sources(&self.evidence, &mut support);
        support.outputs.push(ArtifactRevision {
            id: format!("{}:review", run.id()),
            logical_name: target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            hash: basecraft_io::hash_bytes(&bytes),
            size: bytes.len() as u64,
            retained: None,
            reference: None,
        });
        crate::records::provenance::publish(run.directory(), &support)?;
        let mut prepared = crate::documents::PreparedWrite::default();
        prepared.check_record::<super::StoredCallSets>(target);
        prepared.push(target, |writer| writer.write_all(&bytes))?;
        prepared.run = Some(run);
        Ok(prepared)
    }

    pub(crate) fn bind_session_review(
        project: Option<&crate::project::Project>,
        target: &Path,
        run: &ArtifactRun,
    ) -> crate::Result<()> {
        let hash = basecraft_io::hash_file(target)?;
        run.bind(project, target, &hash)
            .map_err(|error| crate::Error::Damaged {
                path: target.into(),
                detail: format!(
                    "review data saved; binding incomplete: {error}; repair evidence: {}",
                    run.directory().display()
                ),
            })
    }
}

impl SangerReview {
    pub(crate) fn session_snapshot(&self, target: &Path) -> crate::Result<super::StoredCallSets> {
        Ok(super::StoredCallSets {
            version: 3,
            selected: self.draft_sha256.clone(),
            sets: std::iter::once(self)
                .chain(self.retained.iter())
                .map(|set| set.stored_single_at(target))
                .collect::<crate::Result<_>>()?,
        })
    }
    pub(crate) fn from_session_snapshot(
        snapshot: &super::StoredCallSets,
        target: &Path,
    ) -> crate::Result<Self> {
        let review = Self::decode_value(serde_json::to_value(snapshot)?, target);
        review.map_err(|detail| crate::Error::Damaged {
            path: target.into(),
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanger_review::test_session::ReviewSession;
    use crate::sanger_review::{ReviewChoice, ReviewOp};

    fn draft(folder: &Path) -> (ReviewSession, PathBuf) {
        std::fs::create_dir_all(folder).unwrap();
        let trace = folder.join("input.ab1");
        std::fs::write(
            &trace,
            b"ABIFsynthetic trace identity, not a biological fixture",
        )
        .unwrap();
        let json = serde_json::json!({"status":"draft_requires_review","engine":"Tracy","version_output":"synthetic","engine_sha256":"a".repeat(64),"mode":"basecall",
            "inputs":[{"read":1,"path":trace,"sha256":basecraft_io::sha256_file(&trace).unwrap(),"orientation":"reverse","calls":{"sequence":"GT","positions":[1,2],"scores":[30,30]}}],
            "consensus":"AC","consensus_length":2,"minimum_overlap":1,"minimum_identity":0.8,"overlaps":[],"aligned_columns":2,"conflict_columns":0,
            "columns":[{"column":1,"consensus":"A","support":[{"read":1,"base":"A","kind":"call"}],"conflict":false,"review":true},{"column":2,"consensus":"C","support":[{"read":1,"base":"C","kind":"call"}],"conflict":false,"review":true}]});
        let path = folder.join("draft.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
        (
            ReviewSession::new(&SangerReview::new(&path).unwrap()),
            trace,
        )
    }

    fn edit(session: &mut ReviewSession) {
        session
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
    }

    #[test]
    fn save_and_move_keep_exact_draft_and_reference_only_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("work");
        let (mut review, trace) = draft(&folder);
        let exact = review.draft_json.clone();
        edit(&mut review);
        let path = folder.join("specimen.sanger-review.json");
        review.save(&path).unwrap();
        assert_eq!(
            crate::records::provenance::verify(&path, None).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(review.evidence.sources.len(), 1);
        let ledger = review.evidence.sources[0].ledger.as_ref().unwrap();
        assert_eq!(
            inputs::read_input(ledger).unwrap().state,
            basecraft_protocol::input_records::EvidenceState::Referenced
        );
        assert!(!ledger.join("preserved-input").exists());
        assert!(
            review.evidence.sources[0]
                .reference
                .as_ref()
                .unwrap()
                .hash
                .starts_with("b3:")
        );
        let moved = tmp.path().join("moved");
        std::fs::rename(&folder, &moved).unwrap();
        let path = moved.join("specimen.sanger-review.json");
        let reopened = ReviewSession::load(&path).unwrap();
        assert_eq!(reopened.draft_json, exact);
        assert_eq!(
            reopened.working_draft().unwrap().inputs[0]
                .path
                .canonicalize()
                .unwrap(),
            moved
                .join(trace.file_name().unwrap())
                .canonicalize()
                .unwrap()
        );
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        assert_eq!(
            crate::records::provenance::verify(&path, None).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn complete_review_export_survives_missing_originals_and_another_move() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("original");
        let (mut review, _) = draft(&folder);
        edit(&mut review);
        let exact = review.draft_json.clone();
        let path = folder.join("specimen.sanger-review.json");
        review.save(&path).unwrap();
        let package = tmp.path().join("package");
        crate::records::provenance::package(&path, &package, None).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let moved = tmp.path().join("moved-export");
        std::fs::rename(&package, &moved).unwrap();
        let path = moved.join("specimen.sanger-review.json");
        let reopened = ReviewSession::load(&path).unwrap();
        assert_eq!(reopened.draft_json, exact);
        assert_eq!(reopened.events(), review.events());
        assert_eq!(reopened.unresolved(), 1);
        assert!(reopened.working_draft().unwrap().inputs[0].path.is_file());
        assert_eq!(
            crate::records::provenance::verify(&path, None).unwrap(),
            Vec::<String>::new()
        );
        let mut saved_again = reopened;
        saved_again
            .save(&moved.join("another.sanger-review.json"))
            .unwrap();
    }

    #[test]
    fn consensus_requires_resolution_and_complete_export_includes_resumable_review() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("original");
        let (mut review, _) = draft(&folder);
        let output = folder.join("consensus.fa");
        assert!(review.export_consensus(&output, "sample").is_err());
        assert!(!output.exists());
        review
            .apply(ReviewOp::Set {
                columns: vec![0, 1],
                choice: ReviewChoice::Accept,
            })
            .unwrap();
        review.export_consensus(&output, "sample").unwrap();
        assert_eq!(
            crate::records::provenance::verify(&output, None).unwrap(),
            Vec::<String>::new()
        );
        let package = tmp.path().join("export");
        crate::records::provenance::package(&output, &package, None).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let resumed = ReviewSession::load(&package.join("consensus.sanger-review.json")).unwrap();
        assert_eq!(resumed.resolved_sequence().as_deref(), Some("AC"));
        assert!(resumed.working_draft().is_ok());
        assert_eq!(
            crate::records::provenance::verify(&package.join("consensus.fa"), None).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_changed_and_explicitly_preserved_sources_are_distinguished() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut review, trace) = draft(tmp.path());
        let path = tmp.path().join("review.sanger-review.json");
        review.save(&path).unwrap();
        let original = std::fs::read(&trace).unwrap();
        std::fs::write(&trace, b"ABIFdifferent bytes").unwrap();
        assert!(ReviewSession::load(&path).unwrap().working_draft().is_err());
        assert!(
            crate::records::provenance::package(&path, &tmp.path().join("bad-export"), None)
                .is_err()
        );
        assert!(!tmp.path().join("bad-export").exists());
        std::fs::remove_file(&trace).unwrap();
        assert!(ReviewSession::load(&path).unwrap().working_draft().is_err());
        std::fs::write(&trace, &original).unwrap();
        inputs::relink(review.evidence.sources[0].ledger.as_ref().unwrap(), &trace).unwrap();
        let conflict = inputs::conflicts(&trace).unwrap();
        assert!(!conflict.analyses.is_empty());
        inputs::resolve(&conflict, inputs::SavePolicy::PreserveCopy).unwrap();
        std::fs::write(&trace, b"ABIFnew user data").unwrap();
        let reopened = ReviewSession::load(&path).unwrap();
        let resolved = reopened.working_draft().unwrap();
        assert_eq!(std::fs::read(&resolved.inputs[0].path).unwrap(), original);
        assert_eq!(std::fs::read(&trace).unwrap(), b"ABIFnew user data");
        crate::records::provenance::package(&path, &tmp.path().join("preserved-export"), None)
            .unwrap();
    }

    #[test]
    fn a_placement_context_inside_the_project_keeps_its_history_link() {
        let tmp = tempfile::tempdir().unwrap();
        let chosen = tmp.path().join("project");
        std::fs::create_dir(&chosen).unwrap();
        let context = chosen.join("context.fasta");
        std::fs::write(&context, b">a\nACGT\n").unwrap();
        let project = crate::project::Project::new(&chosen).unwrap();
        let mut session = crate::Session::new(&chosen).in_project(Some(project.clone()));
        let doc = session.open(&context).unwrap();
        session
            .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
            .unwrap();
        session.save(doc, None).unwrap();
        let sha256 = basecraft_io::sha256_file(&context).unwrap();
        let placed = source(
            &context,
            Some(&sha256),
            &chosen.join("review.json"),
            Some(&project),
        )
        .unwrap();
        let upstream = inputs::read_input(placed.ledger.as_deref().unwrap())
            .unwrap()
            .upstream;
        assert_eq!(upstream.len(), 1, "{upstream:?}");
        assert_eq!(
            upstream[0].bundle.file_name(),
            project.provenance_bundle(&context).unwrap().file_name()
        );
    }

    #[test]
    fn failed_analysis_keeps_owned_partial_output_and_declares_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, trace) = draft(tmp.path());
        let run =
            SangerRun::prepare(&trace, "consensus", std::slice::from_ref(&trace), None).unwrap();
        let partial = run.artifact("partial-draft").unwrap();
        std::fs::write(&partial, b"partial").unwrap();
        run.failed("cancelled by user before review publication")
            .unwrap();
        assert!(partial.is_file());
        let job: serde_json::Value =
            serde_json::from_slice(&std::fs::read(run.artifact("runtime.json").unwrap()).unwrap())
                .unwrap();
        assert_eq!(job["state"], "failed");
        assert_eq!(job["artifacts"], serde_json::json!([]));
        assert!(trace.is_file());
    }

    #[test]
    fn multiple_read_and_placement_evidence_export_with_exact_original_mappings() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("original");
        let (first, trace) = draft(&folder);
        let other = folder.join("other.ab1");
        std::fs::write(&other, b"ABIFsecond synthetic trace").unwrap();
        let mut raw: serde_json::Value = serde_json::from_str(&first.draft_json).unwrap();
        raw["mode"] = "consensus".into();
        raw["inputs"].as_array_mut().unwrap().push(serde_json::json!({"read":2,"path":other,"sha256":basecraft_io::sha256_file(&other).unwrap(),"orientation":"forward","calls":{"sequence":"AC","positions":[3,4],"scores":[20,30]}}));
        let run = SangerRun::prepare(&trace, "consensus", &[trace.clone(), other], None).unwrap();
        let evidence = run.artifact("evidence.json").unwrap();
        inputs::write_json(&evidence, &raw).unwrap();
        let reads = run.artifact("reads.align.fa").unwrap();
        std::fs::write(&reads, ">r1\nAC\n>r2\nAC\n").unwrap();
        let run = run.finish(&[evidence.clone(), reads]).unwrap();
        // The stage leaves exactly one tool run, which points at its run record.
        let (publication, _) = crate::records::latest_record(run.directory()).unwrap();
        let tool_runs: Vec<_> = publication
            .events
            .iter()
            .filter_map(|e| e.op.tool_run())
            .collect();
        assert_eq!(tool_runs.len(), 1);
        assert_eq!(tool_runs[0].0, "consensus");
        assert!(publication.events.iter().any(|e| e.op.is_run(run.run.id())));
        let record: basecraft_protocol::records::RunRecord =
            serde_json::from_slice(&std::fs::read(run.directory().join("execution.json")).unwrap())
                .unwrap();
        assert_eq!(record.operation, "basecraft.sanger-review/consensus");
        let mut review = ReviewSession::new(&SangerReview::new(&evidence).unwrap());
        review.attach_run(&run).unwrap();
        edit(&mut review);
        let alignment = folder.join("context.afa");
        std::fs::write(&alignment, ">a\nAC\n>b\nAC\n").unwrap();
        let placement_run = SangerRun::prepare_with_dependencies(
            &trace,
            "place_sanger",
            std::slice::from_ref(&alignment),
            review.evidence_runs(),
            None,
        )
        .unwrap();
        let placement_path = placement_run.artifact("placement.json").unwrap();
        let placement = SangerPlacement {
            status: "candidate_requires_review".into(),
            engine: "MAFFT".into(),
            version: "synthetic".into(),
            source_path: alignment.clone(),
            source_sha256: basecraft_io::sha256_file(&alignment).unwrap(),
            source_rows: 2,
            source_width: 2,
            fragment: "AC".into(),
            map: vec![Some(0), Some(1)],
            aligned_probe: "AC".into(),
            insertions: 0,
            parameters: vec!["--mapout".into()],
        };
        inputs::write_json(&placement_path, &placement).unwrap();
        let placement_run = placement_run
            .finish(std::slice::from_ref(&placement_path))
            .unwrap();
        review
            .attach_placement(&placement_run, &placement_path)
            .unwrap();
        let path = folder.join("multi.sanger-review.json");
        review.save(&path).unwrap();
        let package = tmp.path().join("export");
        crate::records::provenance::package(&path, &package, None).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let reopened = ReviewSession::load(&package.join("multi.sanger-review.json")).unwrap();
        assert_eq!(reopened.draft_json, review.draft_json);
        assert_eq!(reopened.working_draft().unwrap().inputs.len(), 2);
        let restored = reopened.placement().unwrap().unwrap();
        assert_eq!(restored.map, placement.map);
        assert_eq!(restored.source_sha256, placement.source_sha256);
        assert!(restored.source_path.is_file());
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
    }

    #[test]
    fn alignment_addition_retains_review_ancestry_and_packages_without_originals() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("original");
        let (mut review, _) = draft(&folder);
        review
            .apply(ReviewOp::Set {
                columns: vec![0, 1],
                choice: ReviewChoice::Accept,
            })
            .unwrap();
        let descriptor = review.addition_evidence(None).unwrap();
        let alignment = folder.join("alignment.afa");
        std::fs::write(&alignment, ">a\nAC\n>b\nAG\n").unwrap();
        let mut session = crate::Session::recoverable(&folder).unwrap();
        let doc = session.open(&alignment).unwrap();
        assert!(record_addition(&mut session, doc, &descriptor, "TT").is_err());
        record_addition(&mut session, doc, &descriptor, "AC").unwrap();
        session
            .apply(
                doc,
                basecraft_protocol::Op::AlnAddSequences {
                    rows: vec![("specimen".into(), "AC".into())],
                    reference: None,
                },
            )
            .unwrap();
        session.undo(doc).unwrap();
        session.redo(doc).unwrap();
        let result = folder.join("added.afa");
        session.save(doc, Some(&result)).unwrap();
        assert_eq!(
            crate::records::provenance::verify(&result, None).unwrap(),
            Vec::<String>::new()
        );
        let package = tmp.path().join("export");
        crate::records::provenance::package(&result, &package, None).unwrap();
        drop(session);
        std::fs::remove_dir_all(&folder).unwrap();
        let result = package.join("added.afa");
        assert_eq!(
            crate::records::provenance::verify(&result, None).unwrap(),
            Vec::<String>::new()
        );
        let mut session = crate::Session::recoverable(&package).unwrap();
        let doc = session.open(&result).unwrap();
        let saved_again = package.join("saved-again.afa");
        let packaged_bytes = std::fs::read(&result).unwrap();
        // Save As to a new path is never blocked by the deleted original project.
        session.save(doc, Some(&saved_again)).unwrap();
        assert_eq!(std::fs::read(&saved_again).unwrap(), packaged_bytes);
        assert_eq!(
            crate::records::provenance::verify(&saved_again, None).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn unsupported_records_and_source_aliases_are_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut review, trace) = draft(tmp.path());
        let path = tmp.path().join("review.json");
        review.save(&path).unwrap();
        let mut extended: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        extended["unknown_field"] = true.into();
        let bytes = serde_json::to_vec(&extended).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // D14: a field this build does not know does not stop it opening.
        assert!(ReviewSession::load(&path).is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let alias = tmp.path().join("trace-alias.json");
        std::fs::hard_link(&trace, &alias).unwrap();
        let original = std::fs::read(&trace).unwrap();
        assert!(review.save(&alias).is_err());
        assert_eq!(std::fs::read(trace).unwrap(), original);
    }

    #[test]
    fn cancellation_and_early_exit_retain_support_without_a_successful_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, trace) = draft(tmp.path());
        for cancelled in [false, true] {
            let run = SangerRun::prepare(&trace, "consensus", std::slice::from_ref(&trace), None)
                .unwrap();
            let directory = run.directory().to_path_buf();
            std::fs::write(run.artifact("partial").unwrap(), b"recover me").unwrap();
            let job = run.artifact("runtime.json").unwrap();
            if cancelled {
                run.cancelled("explicit cancellation").unwrap();
                assert!(
                    run.finish(&[directory.join("partial")]).is_err(),
                    "late results must not publish after cancellation"
                );
            } else {
                drop(run);
            }
            assert_eq!(
                std::fs::read(directory.join("partial")).unwrap(),
                b"recover me"
            );
            let record: serde_json::Value =
                serde_json::from_slice(&std::fs::read(job).unwrap()).unwrap();
            assert_eq!(
                record["state"],
                if cancelled {
                    "cancelled"
                } else {
                    "interrupted"
                }
            );
            assert!(record["primary_output"].is_null());
        }
    }
}
