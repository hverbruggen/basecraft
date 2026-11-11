//! Sanger adapters for the common artifact store and one-source evidence ledgers.
use super::{SangerReview, StoredReview};
use crate::{analysis_store::ArtifactRun, error::invalid, input_evidence as inputs};
use basecraft_docs::sanger::{SangerDraft, SangerPlacement};
use basecraft_protocol::provenance::{ArtifactRevision, Completeness, Publication};
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

fn source(path: &Path, sha256: &str, anchor: &Path) -> crate::Result<Source> {
    let path = path.canonicalize()?;
    let hash = basecraft_io::hash_file(&path)?;
    let ledger = inputs::reference_input_with_dependencies(
        anchor,
        &path,
        &hash,
        &[],
        "Explicit Sanger input reference",
    )?;
    // Check both identities around registration, never label a SHA-256 as BLAKE3.
    if !basecraft_io::sha256_file(&path)?.eq_ignore_ascii_case(sha256) {
        return Err(invalid("Sanger input changed while registering evidence"));
    }
    let reference = inputs::read_input(ledger.directory())?.source;
    Ok(Source {
        extra: BTreeMap::new(),
        original: path.clone(),
        path,
        sha256: sha256.into(),
        ledger: Some(ledger.directory().into()),
        reference,
    })
}

pub(super) trait SourceAccess {
    fn checked(&self) -> crate::Result<PathBuf>;
}

impl SourceAccess for Source {
    fn checked(&self) -> crate::Result<PathBuf> {
        let path = match (&self.ledger, &self.reference) {
            (Some(ledger), Some(reference)) => match inputs::verified_source(ledger, reference) {
                Ok(path) => path,
                // A moved working folder can retain the exact original bytes
                // even when an application-state ledger still names the old location.
                Err(error) => {
                    if matches!(&error, crate::Error::Io(e) | crate::Error::Format(basecraft_io::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound)
                        && inputs::read_input(ledger)?.state == "referenced"
                        && basecraft_io::sha256_file(&self.path)
                            .is_ok_and(|hash| hash.eq_ignore_ascii_case(&self.sha256))
                    {
                        self.path.clone()
                    } else {
                        return Err(error);
                    }
                }
            },
            (None, None) => self.path.clone(),
            _ => return Err(invalid("Incomplete Sanger input reference")),
        };
        if !basecraft_io::sha256_file(&path)?.eq_ignore_ascii_case(&self.sha256) {
            return Err(invalid(format!(
                "Sanger source missing or changed: {}",
                self.original.display()
            )));
        }
        Ok(path)
    }
}

/// An explicit Sanger analysis reserves durable support before invoking a tool.
/// Errors/cancellation never remove its directory or an original input.
pub struct SangerRun {
    run: ArtifactRun,
    evidence: ReviewEvidence,
    operation: String,
    anchor: PathBuf,
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

fn carry_authored(dependencies: &[PathBuf], run: &ArtifactRun) -> crate::Result<()> {
    for dependency in dependencies {
        let authored = dependency.join("reports/methods.md");
        if !authored.is_file() {
            continue;
        }
        if std::fs::symlink_metadata(&authored)?
            .file_type()
            .is_symlink()
        {
            return Err(invalid(
                "Authored methods path is a symlink; original preserved",
            ));
        }
        std::fs::create_dir_all(run.directory().join("reports"))?;
        basecraft_io::write_atomic(&run.directory().join("reports/methods.md"), |w| {
            std::io::copy(&mut std::fs::File::open(&authored)?, w)?;
            Ok(())
        })?;
        break;
    }
    Ok(())
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
    pub fn prepare(anchor: &Path, operation: &str, paths: &[PathBuf]) -> crate::Result<Self> {
        Self::prepare_with_dependencies(anchor, operation, paths, &[])
    }

    pub fn prepare_with_dependencies(
        anchor: &Path,
        operation: &str,
        paths: &[PathBuf],
        ancestors: &[PathBuf],
    ) -> crate::Result<Self> {
        if paths.is_empty() || paths.len() > 4 {
            return Err(invalid("Sanger requires one to four input files"));
        }
        let mut evidence = ReviewEvidence::default();
        for path in paths {
            let sha256 = basecraft_io::sha256_file(path)?;
            evidence.sources.push(source(path, &sha256, anchor)?);
        }
        let run = ArtifactRun::create(anchor, &ulid::Ulid::generate().to_string())?;
        let mut dependencies: Vec<_> = evidence
            .sources
            .iter()
            .filter_map(|s| s.ledger.clone())
            .collect();
        dependencies.extend_from_slice(ancestors);
        let payload =
            serde_json::to_vec(&serde_json::json!({"operation":operation,"sources":paths}))?;
        inputs::write_json(
            &run.artifact("request.json")?,
            &serde_json::from_slice::<serde_json::Value>(&payload)?,
        )?;
        linked_input(&run, &dependencies, &payload)?;
        carry_authored(ancestors, &run)?;
        let result = Self {
            run,
            evidence,
            operation: operation.into(),
            anchor: std::path::absolute(anchor)?,
            terminal: std::cell::Cell::new(false),
            started: basecraft_platform::now_rfc3339(),
        };
        result.status("running", None, None)?;
        Ok(result)
    }

    pub fn parameters(&self, value: &serde_json::Value) -> crate::Result<()> {
        inputs::write_json(&self.run.artifact("parameters.json")?, value)
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

    fn job_path(&self) -> crate::Result<PathBuf> {
        job_path(self.run.id())
    }

    fn status(
        &self,
        status: &str,
        result: Option<&Path>,
        diagnostic: Option<&str>,
    ) -> crate::Result<()> {
        let value = serde_json::json!({"schema_version":1,"id":self.run.id(),"owner_instance":basecraft_platform::process_instance(),
            "status":status,"operation":self.operation,"started":self.started,
            "source":{"path":self.evidence.sources.first().map(|s| &s.path)},"outputs":{},"primary_output":result,
            "diagnostics":diagnostic,"artifact_run":self.run.directory()});
        inputs::write_json(&self.run.artifact("execution.json")?, &value)?;
        let path = self.job_path()?;
        std::fs::create_dir_all(path.parent().ok_or_else(|| invalid("job has no parent"))?)?;
        inputs::write_json(&path, &value)
    }

    pub fn failed(&self, diagnostic: &str) -> crate::Result<()> {
        if self.terminal.get() {
            return Err(invalid("Sanger run already terminated"));
        }
        self.status("failed", None, Some(diagnostic))?;
        self.terminal.set(true);
        Ok(())
    }

    pub fn cancelled(&self, diagnostic: &str) -> crate::Result<()> {
        if self.terminal.get() {
            return Err(invalid("Sanger run already terminated"));
        }
        self.status("cancelled", None, Some(diagnostic))?;
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
        let mut paths = paths.to_vec();
        let parameters = self.run.artifact("parameters.json")?;
        if parameters.is_file() {
            paths.push(parameters);
        }
        for (index, path) in paths.iter().enumerate() {
            artifacts.push(retained(&self.run, path, index + 1)?);
        }
        for source in &self.evidence.sources {
            source.checked()?;
        }
        crate::provenance::publish(
            self.directory(),
            &publication(
                &self.run,
                &format!("Sanger {} support; not accepted consensus", self.operation),
                artifacts,
            ),
        )?;
        self.evidence.runs.push(self.directory().into());
        self.status("support_retained", None, None)?;
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
                "interrupted",
                None,
                Some("Sanger support was not published; owned files retained"),
            );
        }
    }
}

fn job_path(id: &str) -> crate::Result<PathBuf> {
    let paths =
        basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
    Ok(paths.state.join("jobs").join(id).join("manifest.json"))
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
        self.anchor.clone_from(&run.anchor);
        Ok(())
    }

    pub fn recovery_path(&self) -> Option<&Path> {
        self.recovery.as_deref()
    }

    pub(super) fn resume_recovery(&mut self, path: &Path) -> Result<(), String> {
        let paths = basecraft_platform::paths::AppPaths::discover().map_err(|e| e.to_string())?;
        let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
        if !absolute.starts_with(paths.state.join("jobs")) {
            return Ok(());
        }
        // State recovery is an unsaved document. Its suggested Save/Export
        // location must remain in the working folder, never in application state.
        let mut folder = None;
        for run in self.evidence.runs.iter().chain(
            self.evidence
                .sources
                .iter()
                .filter_map(|s| s.ledger.as_ref()),
        ) {
            let Ok(run) = run.canonicalize() else {
                continue;
            };
            let candidate = if let Some(store) = run
                .parent()
                .and_then(Path::parent)
                .filter(|p| p.file_name().is_some_and(|n| n == "analyses-v1"))
            {
                if ArtifactRun::validate_store(store).is_err() {
                    continue;
                }
                store.parent().and_then(Path::parent)
            } else if run
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".provenance"))
            {
                run.parent()
            } else {
                None
            };
            if let Some(candidate) = candidate.filter(|p| !p.starts_with(&paths.state)) {
                folder = Some(candidate.to_path_buf());
                break;
            }
        }
        let folder = match folder {
            Some(folder) => folder,
            None => std::env::current_dir().map_err(|e| e.to_string())?,
        };
        self.anchor = folder.join("recovered-review.sanger-review.json");
        self.accepted = None;
        self.recovery = Some(absolute);
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
        let mut copy = self.clone();
        copy.recovery = None;
        copy
    }

    /// Carry already validated review ancestry, including pre-registry reviews.
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
    ) -> Result<(), String> {
        let placement = SangerPlacement::read(path)?;
        let context = source(
            &placement.source_path,
            &placement.source_sha256,
            &self.anchor,
        )
        .map_err(|e| e.to_string())?;
        self.evidence.sources.push(context);
        self.evidence.placement = Some(Support {
            extra: BTreeMap::new(),
            path: path.into(),
            hash: basecraft_io::hash_file(path).map_err(|e| e.to_string())?,
        });
        self.attach_operation_run(directory)
    }

    pub fn evidence_runs(&self) -> &[PathBuf] {
        &self.evidence.runs
    }

    pub fn include_run(&mut self, run: &SangerRun) -> Result<(), String> {
        self.evidence.runs.extend(run.evidence.runs.clone());
        Ok(())
    }

    /// Resolve live/preserved/packaged paths into a working copy. Immutable draft
    /// JSON and its SHA-256 remain unchanged, including original path spellings.
    pub fn working_draft(&self) -> Result<SangerDraft, String> {
        let mut draft = self.draft.clone();
        for read in &mut draft.inputs {
            if let Some(source) =
                self.evidence.sources.iter().find(|s| {
                    s.original == read.path && s.sha256.eq_ignore_ascii_case(&read.sha256)
                })
            {
                read.path = source.checked().map_err(|e| {
                    format!(
                        "Sanger original unavailable at {}: {e}",
                        source.original.display()
                    )
                })?;
            }
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
                extra: BTreeMap::new(),
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
        for input in &mut self.evidence.sources {
            if input.ledger.is_none()
                && basecraft_io::sha256_file(&input.path)
                    .is_ok_and(|h| h.eq_ignore_ascii_case(&input.sha256))
            {
                let original = input.original.clone();
                *input = source(&input.path, &input.sha256, anchor)?;
                input.original = original;
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
            extra: BTreeMap::new(),
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
            version: 1,
            draft_path: self.draft_path.clone(),
            draft_sha256: self.draft_sha256.clone(),
            draft_json: self.draft_json.clone(),
            events: self.events.clone(),
            evidence,
            extra: self.stored_extra.clone(),
        })
    }

    #[cfg(test)]
    fn stored_at(&self, path: &Path) -> crate::Result<serde_json::Value> {
        self.stored_with_evidence(path, &self.evidence)
    }

    fn stored_with_evidence(
        &self,
        path: &Path,
        evidence: &ReviewEvidence,
    ) -> crate::Result<serde_json::Value> {
        if self.retained.is_empty() {
            return Ok(serde_json::to_value(
                self.stored_single_with_evidence(path, evidence)?,
            )?);
        }
        let mut sets = vec![self.stored_single_with_evidence(path, evidence)?];
        for set in &self.retained {
            sets.push(set.stored_single_at(path)?);
        }
        Ok(serde_json::to_value(super::StoredCallSets {
            version: 2,
            selected: self.draft_sha256.clone(),
            sets,
        })?)
    }

    fn write_embedded(&self, path: &Path) -> crate::Result<()> {
        let mut bytes = self.encoded_at(path, &self.evidence)?;
        // Embedded recovery/support JSON historically includes a final newline.
        bytes.push(b'\n');
        basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
        Ok(())
    }

    fn encoded_at(&self, path: &Path, evidence: &ReviewEvidence) -> crate::Result<Vec<u8>> {
        let bytes = serde_json::to_vec_pretty(&self.stored_with_evidence(path, evidence)?)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(invalid(
                "Sanger review exceeds 64 MiB; previous file preserved",
            ));
        }
        Ok(bytes)
    }

    pub(super) fn publish_consensus(&mut self, path: &Path) -> crate::Result<()> {
        self.ensure_sources(path)?;
        let run = ArtifactRun::create(path, &ulid::Ulid::generate().to_string())?;
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
        carry_authored(&dependencies, &run)?;
        let snapshot = run.artifact("review.sanger-review.json")?;
        self.write_embedded(&snapshot)?;
        let mut record = publication(
            &run,
            "Sanger accepted consensus; full review evidence retained",
            vec![retained(&run, &snapshot, 0)?],
        );
        unavailable_sources(&self.evidence, &mut record);
        record.outputs.push(ArtifactRevision {
            id: format!("{}:consensus", run.id()),
            logical_name: path
                .file_name()
                .ok_or_else(|| invalid("Consensus has no filename"))?
                .to_string_lossy()
                .into_owned(),
            hash: basecraft_io::hash_file(path)?,
            size: std::fs::metadata(path)?.len(),
            retained: None,
            reference: None,
        });
        crate::provenance::publish(run.directory(), &record)?;
        run.bind(path)?;
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
    let bytes = crate::provenance::read_record(copied)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let value = match value["version"].as_u64() {
        Some(1) => {
            let mut stored: StoredReview = serde_json::from_value(value)?;
            relocate_stored(&mut stored, original, copied, bundle, paths)?;
            serde_json::to_value(stored)?
        }
        Some(2) => {
            let mut stored: super::StoredCallSets = serde_json::from_value(value)?;
            for set in &mut stored.sets {
                relocate_stored(set, original, copied, bundle, paths)?;
            }
            serde_json::to_value(stored)?
        }
        _ => return Err(invalid("Unsupported Sanger review version")),
    };
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
    pub fn addition_evidence(&mut self) -> Result<String, String> {
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
        )
        .map_err(|e| e.to_string())?;
        let review = run.review_snapshot(self).map_err(|e| e.to_string())?;
        let run = run
            .finish(std::slice::from_ref(&review))
            .map_err(|e| e.to_string())?;
        let bytes = crate::provenance::read_record(&run.directory().join("manifest.json"))
            .map_err(|e| e.to_string())?;
        let evidence = AdditionEvidence {
            schema_version: 1,
            bundle: run.directory().into(),
            id: run.run.id().into(),
            hash: basecraft_io::hash_bytes(&bytes),
            review: review.clone(),
            review_hash: basecraft_io::hash_file(&review).map_err(|e| e.to_string())?,
            sequence_hash: basecraft_io::hash_bytes(sequence.as_bytes()),
        };
        serde_json::to_string(&evidence).map_err(|e| e.to_string())
    }
}

/// Verify the exact accepted review before selecting it as an alignment input.
/// The actual row mutation continues through the existing typed alignment op.
pub fn record_addition(
    session: &mut crate::Session,
    doc: crate::DocId,
    descriptor: &str,
    sequence: &str,
) -> crate::Result<()> {
    let evidence: AdditionEvidence = serde_json::from_str(descriptor)?;
    if evidence.schema_version != 1
        || evidence.sequence_hash != basecraft_io::hash_bytes(sequence.as_bytes())
    {
        return Err(invalid(
            "Reviewed sequence differs from its Sanger evidence",
        ));
    }
    let bytes = crate::provenance::read_record(&evidence.bundle.join("manifest.json"))?;
    let publication: Publication = serde_json::from_slice(&bytes)?;
    if publication.id != evidence.id
        || basecraft_io::hash_bytes(&bytes) != evidence.hash
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
    let mut issues = crate::provenance::verify_records(&evidence.bundle, &publication)?;
    issues.extend(inputs::verify_ancestors(&evidence.bundle)?);
    if !issues.is_empty() {
        return Err(invalid(issues.join("; ")));
    }
    let hash = session.snapshot_file(&evidence.review)?;
    session.log_referenced_tool_run(doc,basecraft_protocol::Actor::Plugin {id:"basecraft.sanger-review".into(),version:"1".into()},
        ("basecraft.sanger-review","reviewed_consensus_input"),
        serde_json::json!({"sanger_input":{"schema_version":1,"id":evidence.id,"hash":evidence.hash},"review_hash":evidence.review_hash,"sequence_hash":evidence.sequence_hash}),
        &[(evidence.review.to_string_lossy().into_owned(),hash)],&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanger_review::{ReviewChoice, ReviewOp};

    fn draft(folder: &Path) -> (SangerReview, PathBuf) {
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
        (SangerReview::new(&path).unwrap(), trace)
    }

    fn edit(session: &mut SangerReview) {
        session
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
    }

    #[test]
    fn save_move_and_undo_keep_exact_draft_and_reference_only_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("work");
        let (mut review, trace) = draft(&folder);
        assert!(
            !folder.join(".basecraft").exists(),
            "opening must not register/copy inputs"
        );
        let exact = review.draft_json.clone();
        edit(&mut review);
        let path = folder.join("specimen.sanger-review.json");
        review.save(&path).unwrap();
        assert_eq!(
            crate::provenance::verify(&path).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(review.evidence.sources.len(), 1);
        let ledger = review.evidence.sources[0].ledger.as_ref().unwrap();
        assert_eq!(inputs::read_input(ledger).unwrap().state, "referenced");
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
        let mut reopened = SangerReview::load(&path).unwrap();
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
        reopened.apply(ReviewOp::Undo).unwrap();
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Pending));
        reopened.apply(ReviewOp::Redo).unwrap();
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        assert_eq!(
            crate::provenance::verify(&path).unwrap(),
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
        let authored = crate::provenance::bundle_for_result(&path).join("reports/methods.md");
        std::fs::write(
            &authored,
            "User-authored laboratory methods, preserved verbatim.\n",
        )
        .unwrap();
        review.save(&path).unwrap();
        let package = tmp.path().join("package");
        crate::provenance::package(&path, &package).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let moved = tmp.path().join("moved-export");
        std::fs::rename(&package, &moved).unwrap();
        let path = moved.join("specimen.sanger-review.json");
        assert_eq!(
            std::fs::read_to_string(crate::provenance::companion(&path).join("reports/methods.md"))
                .unwrap(),
            "User-authored laboratory methods, preserved verbatim.\n"
        );
        let reopened = SangerReview::load(&path).unwrap();
        assert_eq!(reopened.draft_json, exact);
        assert_eq!(reopened.events(), review.events());
        assert_eq!(reopened.unresolved(), 1);
        assert!(reopened.working_draft().unwrap().inputs[0].path.is_file());
        assert_eq!(
            crate::provenance::verify(&path).unwrap(),
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
            crate::provenance::verify(&output).unwrap(),
            Vec::<String>::new()
        );
        let package = tmp.path().join("export");
        crate::provenance::package(&output, &package).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let resumed = SangerReview::load(&package.join("consensus.sanger-review.json")).unwrap();
        assert_eq!(resumed.resolved_sequence().as_deref(), Some("AC"));
        assert!(resumed.working_draft().is_ok());
        assert_eq!(
            crate::provenance::verify(&package.join("consensus.fa")).unwrap(),
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
        assert!(SangerReview::load(&path).unwrap().working_draft().is_err());
        assert!(crate::provenance::package(&path, &tmp.path().join("bad-export")).is_err());
        assert!(!tmp.path().join("bad-export").exists());
        std::fs::remove_file(&trace).unwrap();
        assert!(SangerReview::load(&path).unwrap().working_draft().is_err());
        std::fs::write(&trace, &original).unwrap();
        inputs::relink(review.evidence.sources[0].ledger.as_ref().unwrap(), &trace).unwrap();
        let conflict = inputs::conflicts(&trace).unwrap();
        assert!(!conflict.analyses.is_empty());
        inputs::resolve(&conflict, inputs::SavePolicy::PreserveCopy).unwrap();
        std::fs::write(&trace, b"ABIFnew user data").unwrap();
        let reopened = SangerReview::load(&path).unwrap();
        let resolved = reopened.working_draft().unwrap();
        assert_eq!(std::fs::read(&resolved.inputs[0].path).unwrap(), original);
        assert_eq!(std::fs::read(&trace).unwrap(), b"ABIFnew user data");
        crate::provenance::package(&path, &tmp.path().join("preserved-export")).unwrap();
    }

    #[test]
    fn failed_analysis_keeps_owned_partial_output_and_declares_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, trace) = draft(tmp.path());
        let run = SangerRun::prepare(&trace, "consensus", std::slice::from_ref(&trace)).unwrap();
        let partial = run.artifact("partial-draft").unwrap();
        std::fs::write(&partial, b"partial").unwrap();
        run.failed("cancelled by user before review publication")
            .unwrap();
        assert!(partial.is_file());
        let job: serde_json::Value =
            serde_json::from_slice(&std::fs::read(run.job_path().unwrap()).unwrap()).unwrap();
        assert_eq!(job["status"], "failed");
        assert!(job["primary_output"].is_null());
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
        let run = SangerRun::prepare(&trace, "consensus", &[trace.clone(), other]).unwrap();
        let evidence = run.artifact("evidence.json").unwrap();
        inputs::write_json(&evidence, &raw).unwrap();
        let reads = run.artifact("reads.align.fa").unwrap();
        std::fs::write(&reads, ">r1\nAC\n>r2\nAC\n").unwrap();
        let run = run.finish(&[evidence.clone(), reads]).unwrap();
        let mut review = SangerReview::new(&evidence).unwrap();
        review.attach_run(&run).unwrap();
        edit(&mut review);
        let alignment = folder.join("context.afa");
        std::fs::write(&alignment, ">a\nAC\n>b\nAC\n").unwrap();
        let placement_run = SangerRun::prepare_with_dependencies(
            &trace,
            "place_sanger",
            std::slice::from_ref(&alignment),
            review.evidence_runs(),
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
        crate::provenance::package(&path, &package).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let reopened = SangerReview::load(&package.join("multi.sanger-review.json")).unwrap();
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
        let descriptor = review.addition_evidence().unwrap();
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
            crate::provenance::verify(&result).unwrap(),
            Vec::<String>::new()
        );
        let package = tmp.path().join("export");
        crate::provenance::package(&result, &package).unwrap();
        drop(session);
        std::fs::remove_dir_all(&folder).unwrap();
        let result = package.join("added.afa");
        assert_eq!(
            crate::provenance::verify(&result).unwrap(),
            Vec::<String>::new()
        );
        let mut session = crate::Session::recoverable(&package).unwrap();
        let doc = session.open(&result).unwrap();
        let saved_again = package.join("saved-again.afa");
        let packaged_bytes = std::fs::read(&result).unwrap();
        let refused = match session.save(doc, Some(&saved_again)).unwrap_err() {
            crate::Error::SaveCheckRequired(check) => check,
            error => panic!("expected incomplete-inspection consent, got {error}"),
        };
        assert!(!refused.evidence().is_complete());
        assert!(!saved_again.exists());
        assert_eq!(std::fs::read(&result).unwrap(), packaged_bytes);

        // Inspect a fresh check, then explicitly acknowledge only this missing
        // store on the same Session/document. Unknown inspection is independent
        // of known-conflict policy; no known conflicts authorize a policy change.
        let check = session.check_save(doc, Some(&saved_again)).unwrap();
        assert_ne!(check.token(), refused.token());
        assert_eq!(check.target(), saved_again);
        assert!(check.evidence().conflict.analyses.is_empty());
        assert!(matches!(
            &check.evidence().issues[..],
            [issue] if issue.code == inputs::InspectionCode::StoreUnavailable
                && issue.relevance == inputs::InspectionRelevance::Unknown
                && issue.location == folder.join(".basecraft/analyses-v1")
        ));
        let decision = session
            .decide_save(
                doc,
                Some(&saved_again),
                check.token(),
                inputs::SavePolicy::Cancel,
                true,
            )
            .unwrap();
        let outcome = session
            .save_with_decision(doc, Some(&saved_again), decision)
            .unwrap();
        assert!(matches!(
            &outcome.warnings[..],
            [crate::session::SaveWarning::EvidenceNotChecked { issues }]
                if issues == &check.evidence().issues
        ));
        assert_eq!(std::fs::read(&saved_again).unwrap(), packaged_bytes);
        assert_eq!(
            crate::provenance::verify(&saved_again).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn save_preserves_extension_metadata_and_refuses_source_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut review, trace) = draft(tmp.path());
        let path = tmp.path().join("review.sanger-review.json");
        review.save(&path).unwrap();
        let mut extended: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        extended["future_envelope"] = serde_json::json!({"text":"keep user data"});
        extended["evidence"]["future_support"] = serde_json::json!([1, 2]);
        extended["evidence"]["sources"][0]["future_binding"] = "keep".into();
        std::fs::write(&path, serde_json::to_vec(&extended).unwrap()).unwrap();
        let mut loaded = SangerReview::load(&path).unwrap();
        let saved = tmp.path().join("another.sanger-review.json");
        loaded.save(&saved).unwrap();
        let roundtrip: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
        assert_eq!(roundtrip["future_envelope"], extended["future_envelope"]);
        assert_eq!(
            roundtrip["evidence"]["future_support"],
            extended["evidence"]["future_support"]
        );
        assert_eq!(
            roundtrip["evidence"]["sources"][0]["future_binding"],
            "keep"
        );
        let alias = tmp.path().join("trace-alias.json");
        std::fs::hard_link(&trace, &alias).unwrap();
        let original = std::fs::read(&trace).unwrap();
        assert!(loaded.save(&alias).is_err());
        assert_eq!(std::fs::read(trace).unwrap(), original);
    }

    #[test]
    fn cancellation_and_early_exit_retain_support_without_a_successful_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, trace) = draft(tmp.path());
        for cancelled in [false, true] {
            let run =
                SangerRun::prepare(&trace, "consensus", std::slice::from_ref(&trace)).unwrap();
            let directory = run.directory().to_path_buf();
            std::fs::write(run.artifact("partial").unwrap(), b"recover me").unwrap();
            let job = run.job_path().unwrap();
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
                record["status"],
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

#[cfg(test)]
#[path = "legacy_fixtures.rs"]
mod legacy_fixtures;

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

    pub(crate) fn write_session_review(
        &self,
        target: &Path,
        logical: &Path,
        publication_context: &crate::documents::WritePublication,
    ) -> crate::Result<Vec<(PathBuf, String)>> {
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
        let run = ArtifactRun::create(target, &ulid::Ulid::generate().to_string())?;
        let mut dependencies: Vec<_> = ArtifactRun::for_result(target)?.into_iter().collect();
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
        carry_authored(&dependencies, &run)?;
        let bytes = self.session_bytes(logical)?;
        if matches!(
            publication_context,
            crate::documents::WritePublication::Create
        ) {
            let mut staged = tempfile::NamedTempFile::new_in(
                target
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?;
            std::io::Write::write_all(staged.as_file_mut(), &bytes)?;
            staged.as_file().sync_all()?;
            staged
                .persist_noclobber(target)
                .map_err(|error| crate::Error::Io(error.error))?;
            basecraft_platform::files::sync_directory(
                target
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?;
        } else {
            basecraft_io::write_atomic(target, |writer| writer.write_all(&bytes))?;
        }
        run.bind(target).map_err(|error| crate::Error::Damaged {
            path: target.into(),
            detail: format!(
                "review data saved; binding incomplete: {error}; repair evidence: {}",
                run.directory().display()
            ),
        })?;
        Ok(vec![(target.to_owned(), basecraft_io::hash_bytes(&bytes))])
    }
}

impl SangerReview {
    pub(crate) fn session_snapshot(&self, target: &Path) -> crate::Result<super::StoredCallSets> {
        Ok(super::StoredCallSets {
            version: 2,
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
        let review =
            if snapshot.sets.len() == 1 && snapshot.sets[0].draft_sha256 == snapshot.selected {
                Self::from_stored(snapshot.sets[0].clone(), target)
            } else {
                Self::decode_value(serde_json::to_value(snapshot)?, target)
            };
        review.map_err(|detail| crate::Error::Damaged {
            path: target.into(),
            detail,
        })
    }
}
