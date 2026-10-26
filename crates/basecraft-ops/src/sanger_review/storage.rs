//! Sanger adapters for the common artifact store and one-source evidence ledgers.
use super::{SangerReviewSession, StoredReview};
use crate::{analysis_store::ArtifactRun, error::invalid, input_evidence as inputs};
use basecraft_docs::sanger::{SangerDraft, SangerPlacement};
use basecraft_protocol::provenance::{ArtifactRevision, Completeness, InputReference, Publication};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct ReviewEvidence {
    #[serde(default)]
    pub runs: Vec<PathBuf>,
    #[serde(default)]
    pub sources: Vec<Source>,
    pub placement: Option<Support>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Source {
    // Identity in the immutable draft/placement, never rebased.
    pub original: PathBuf,
    pub sha256: String,
    // Location relative to the saved review; ledger controls preserve/break decisions.
    pub path: PathBuf,
    pub ledger: Option<PathBuf>,
    pub reference: Option<InputReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Support {
    pub path: PathBuf,
    pub hash: String,
}

impl ReviewEvidence {
    pub fn map_paths(
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
    Ok(path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?)
}

fn source(path: &Path, sha256: &str, anchor: &Path) -> crate::Result<Source> {
    let path = path.canonicalize()?;
    let hash = basecraft_io::hash_file(&path)?;
    let ledger =
        inputs::reference_input(anchor, &path, &hash, &[], "Explicit Sanger input reference")?;
    // Check both identities around registration, never label a SHA-256 as BLAKE3.
    if !basecraft_io::sha256_file(&path)?.eq_ignore_ascii_case(sha256) {
        return Err(invalid("Sanger input changed while registering evidence"));
    }
    let reference = inputs::read_input(ledger.directory())?.source;
    Ok(Source {
        original: path.clone(),
        path,
        sha256: sha256.into(),
        ledger: Some(ledger.directory().into()),
        reference,
    })
}

impl Source {
    fn checked(&self) -> crate::Result<PathBuf> {
        let path = match (&self.ledger, &self.reference) {
            (Some(ledger), Some(reference)) => inputs::verified_source(ledger, reference)?,
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
}

fn linked_input(run: &ArtifactRun, dependencies: &[PathBuf], payload: &[u8]) -> crate::Result<()> {
    let mut upstream = Vec::new();
    for dependency in dependencies {
        let bytes = crate::provenance::read_record(&dependency.join("manifest.json"))?;
        let publication: Publication = serde_json::from_slice(&bytes)?;
        upstream.push(serde_json::json!({"bundle":inputs::relative(run.directory(), dependency)?, "id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)}));
    }
    inputs::write_json(
        &run.artifact("input.json")?,
        &inputs::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: basecraft_io::hash_bytes(payload),
            submitted_size: payload.len() as u64,
            transformation: "identity-v1".into(),
            state: "generated".into(),
            extra: BTreeMap::from([("upstream".into(), serde_json::json!(upstream))]),
        },
    )?;
    inputs::write_json(
        &run.artifact("run.json")?,
        &serde_json::json!({"schema_version":1,"id":run.id(),"result":"Sanger review/support","status":"prepared"}),
    )?;
    let _lock = inputs::lock()?;
    inputs::register(run)
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
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid("Sanger support is a symlink"));
    }
    std::fs::File::open(path)?.sync_all()?;
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
        let result = Self {
            run,
            evidence,
            operation: operation.into(),
            anchor: std::path::absolute(anchor)?,
        };
        result.status("running", None, None)?;
        Ok(result)
    }

    pub fn parameters(&self, value: &serde_json::Value) -> crate::Result<()> {
        inputs::write_json(&self.run.artifact("parameters.json")?, value)
    }

    pub fn review_snapshot(&self, session: &SangerReviewSession) -> crate::Result<PathBuf> {
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
            "status":status,"operation":self.operation,"started":basecraft_platform::now_rfc3339(),
            "source":{"path":self.evidence.sources.first().map(|s| &s.path)},"outputs":{},"primary_output":result,
            "diagnostics":diagnostic,"artifact_run":self.run.directory()});
        inputs::write_json(&self.run.artifact("execution.json")?, &value)?;
        let path = self.job_path()?;
        std::fs::create_dir_all(path.parent().ok_or_else(|| invalid("job has no parent"))?)?;
        inputs::write_json(&path, &value)
    }

    pub fn failed(&self, diagnostic: &str) -> crate::Result<()> {
        self.status("failed", None, Some(diagnostic))
    }

    /// Retain only declared, checked output files. The invocation directory itself
    /// stays durable even if a plugin fails before returning its declaration.
    pub fn finish(mut self, paths: &[PathBuf]) -> crate::Result<Self> {
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
        Ok(self)
    }
}

fn job_path(id: &str) -> crate::Result<PathBuf> {
    let paths =
        basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
    Ok(paths.state.join("jobs").join(id).join("manifest.json"))
}

impl SangerReviewSession {
    /// Attach an explicit completed analysis. The exact draft itself is unchanged.
    pub fn attach_run(&mut self, run: &SangerRun) -> Result<(), String> {
        let mut evidence = run.evidence.clone();
        for read in &self.draft.inputs {
            let source = evidence
                .sources
                .iter_mut()
                .find(|s| s.sha256.eq_ignore_ascii_case(&read.sha256))
                .ok_or("Draft input differs from registered Sanger input")?;
            source.original = read.path.clone();
        }
        self.evidence = evidence;
        self.anchor = run.anchor.clone();
        self.start_recovery()
    }

    pub fn storage_anchor(&self) -> &Path {
        &self.anchor
    }

    pub fn evidence_runs(&self) -> &[PathBuf] {
        &self.evidence.runs
    }

    pub fn include_run(&mut self, run: &SangerRun) -> Result<(), String> {
        self.evidence.runs.extend(run.evidence.runs.clone());
        self.start_recovery()
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
                read.path = source.checked().map_err(|e| e.to_string())?;
            }
        }
        Ok(draft)
    }

    fn ensure_sources(&mut self, anchor: &Path) -> crate::Result<()> {
        for read in &self.draft.inputs {
            if !self
                .evidence
                .sources
                .iter()
                .any(|s| s.original == read.path && s.sha256.eq_ignore_ascii_case(&read.sha256))
            {
                let candidate = if read.path.is_absolute() {
                    read.path.clone()
                } else {
                    base(anchor)?.join(&read.path)
                };
                let mut input = if basecraft_io::sha256_file(&candidate)
                    .is_ok_and(|h| h.eq_ignore_ascii_case(&read.sha256))
                {
                    source(&candidate, &read.sha256, anchor)?
                } else {
                    // Saving/recovering decisions remains possible without the
                    // original. No ledger may claim these unavailable bytes.
                    Source {
                        original: read.path.clone(),
                        path: candidate,
                        sha256: read.sha256.clone(),
                        ledger: None,
                        reference: None,
                    }
                };
                input.original = read.path.clone();
                self.evidence.sources.push(input);
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
                s.original == placement.source_path
                    && s.sha256.eq_ignore_ascii_case(&placement.source_sha256)
            })
            .ok_or("Placement source differs from registered alignment")?;
        self.evidence.sources.push(context.clone());
        self.evidence.runs.extend(run.evidence.runs.clone());
        self.evidence.placement = Some(Support {
            path: path.into(),
            hash: basecraft_io::hash_file(path).map_err(|e| e.to_string())?,
        });
        self.start_recovery()
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
                s.original == placement.source_path
                    && s.sha256.eq_ignore_ascii_case(&placement.source_sha256)
            })
            .ok_or("Placement input reference is missing")?;
        placement.source_path = context.checked().map_err(|e| e.to_string())?;
        Ok(Some(placement))
    }

    fn stored_at(&self, path: &Path) -> crate::Result<StoredReview> {
        let mut evidence = self.evidence.clone();
        let parent = base(path)?;
        evidence.map_paths(|p| Ok(PathBuf::from(inputs::relative(&parent, p)?)))?;
        Ok(StoredReview {
            version: 1,
            draft_path: self.draft_path.clone(),
            draft_sha256: self.draft_sha256.clone(),
            draft_json: self.draft_json.clone(),
            events: self.events.clone(),
            evidence,
        })
    }

    fn write_embedded(&self, path: &Path) -> crate::Result<()> {
        inputs::write_json(path, &self.stored_at(path)?)
    }

    /// Recovery lives in application state and is discoverable through the existing
    /// Tasks previous-runs list. It is labelled as a review, never a consensus.
    pub fn start_recovery(&mut self) -> Result<(), String> {
        let anchor = self.anchor.clone();
        self.ensure_sources(&anchor).map_err(|e| e.to_string())?;
        if self.recovery.is_none() {
            let path = job_path(&ulid::Ulid::generate().to_string()).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(path.parent().ok_or("Recovery path has no parent")?)
                .map_err(|e| e.to_string())?;
            self.recovery = Some(path.with_file_name("review.sanger-review.json"));
        }
        self.checkpoint()
    }

    pub(super) fn checkpoint(&self) -> Result<(), String> {
        let Some(path) = &self.recovery else {
            return Ok(());
        };
        self.write_embedded(path).map_err(|e| e.to_string())?;
        let directory = path.parent().ok_or("Recovery has no parent")?;
        let manifest = directory.join("manifest.json");
        inputs::write_json(&manifest, &serde_json::json!({"schema_version":1,"id":directory.file_name().unwrap_or_default().to_string_lossy(),
            "owner_instance":basecraft_platform::process_instance(),"operation":"Sanger review recovery","status":"review_requires_attention",
            "started":basecraft_platform::now_rfc3339(),"source":{"path":self.anchor},"outputs":{},"primary_output":path,
            "diagnostics":format!("{} unresolved columns; recoverable review, not accepted consensus", self.unresolved())})).map_err(|e|e.to_string())
    }

    pub(super) fn save_durable(&mut self, path: &Path) -> crate::Result<()> {
        // Never replace an input, even through a symlink or alternate spelling.
        let absolute = std::path::absolute(path)?;
        let destination = path.canonicalize().unwrap_or_else(|_| absolute.clone());
        for read in &self.draft.inputs {
            if read
                .path
                .canonicalize()
                .unwrap_or_else(|_| read.path.clone())
                == destination
            {
                return Err(invalid("Review cannot replace a source AB1"));
            }
        }
        self.ensure_sources(path)?;
        self.start_recovery().map_err(invalid)?;
        let run = ArtifactRun::create(path, &ulid::Ulid::generate().to_string())?;
        let mut dependencies = self.evidence.runs.clone();
        dependencies.extend(
            self.evidence
                .sources
                .iter()
                .filter_map(|s| s.ledger.clone()),
        );
        dependencies.sort();
        dependencies.dedup();
        linked_input(&run, &dependencies, self.draft_json.as_bytes())?;
        self.evidence.runs = vec![run.directory().into()];
        let snapshot = run.artifact("review.sanger-review.json")?;
        // The recovery copy is complete before attempting a visible replacement.
        self.write_embedded(&snapshot)?;
        let mut artifacts = vec![retained(&run, &snapshot, 0)?];
        if path.is_file() {
            let previous = run.artifact("previous-review")?;
            basecraft_io::write_atomic(&previous, |w| {
                std::io::copy(&mut std::fs::File::open(path)?, w)?;
                Ok(())
            })?;
            artifacts.push(retained(&run, &previous, 1)?);
        }
        let bytes = serde_json::to_vec_pretty(&self.stored_at(path)?)?;
        let staged = run.staging("review")?;
        basecraft_io::write_atomic(&staged, |w| w.write_all(&bytes))?;
        let mut record = publication(
            &run,
            "Explicit saved Sanger review; decisions and support retained",
            artifacts,
        );
        record.outputs.push(ArtifactRevision {
            id: format!("{}:review", run.id()),
            logical_name: path
                .file_name()
                .ok_or_else(|| invalid("Review has no filename"))?
                .to_string_lossy()
                .into_owned(),
            hash: basecraft_io::hash_bytes(&bytes),
            size: bytes.len() as u64,
            retained: None,
            reference: None,
        });
        crate::provenance::publish(run.directory(), &record)?;
        basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
        run.bind(path)?;
        self.anchor = absolute;
        self.checkpoint().map_err(invalid)
    }

    pub(super) fn publish_consensus(&mut self, path: &Path) -> crate::Result<()> {
        self.ensure_sources(path)?;
        let run = ArtifactRun::create(path, &ulid::Ulid::generate().to_string())?;
        let mut dependencies = self.evidence.runs.clone();
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
        self.checkpoint().map_err(invalid)
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
    let mut stored: StoredReview = serde_json::from_slice(&bytes)?;
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
    inputs::write_json(copied, &stored)
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
    path.extension().is_some_and(|e| e == "json")
        && crate::provenance::read_record(path)
            .ok()
            .is_some_and(|bytes| serde_json::from_slice::<StoredReview>(&bytes).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanger_review::{ReviewChoice, ReviewOp};

    fn draft(folder: &Path) -> (SangerReviewSession, PathBuf) {
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
        (SangerReviewSession::new(&path).unwrap(), trace)
    }

    fn edit(session: &mut SangerReviewSession) {
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
        assert!(crate::provenance::verify(&path).unwrap().is_empty());
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
        let mut reopened = SangerReviewSession::load(&path).unwrap();
        assert_eq!(reopened.draft_json, exact);
        assert_eq!(
            reopened.working_draft().unwrap().inputs[0]
                .path
                .canonicalize()
                .unwrap(),
            moved.join(trace.file_name().unwrap())
        );
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        reopened.apply(ReviewOp::Undo).unwrap();
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Pending));
        reopened.apply(ReviewOp::Redo).unwrap();
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        assert!(crate::provenance::verify(&path).unwrap().is_empty());
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
        crate::provenance::package(&path, &package).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let moved = tmp.path().join("moved-export");
        std::fs::rename(&package, &moved).unwrap();
        let path = moved.join("specimen.sanger-review.json");
        let reopened = SangerReviewSession::load(&path).unwrap();
        assert_eq!(reopened.draft_json, exact);
        assert_eq!(reopened.events(), review.events());
        assert_eq!(reopened.unresolved(), 1);
        assert!(reopened.working_draft().unwrap().inputs[0].path.is_file());
        assert!(crate::provenance::verify(&path).unwrap().is_empty());
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
        assert!(crate::provenance::verify(&output).unwrap().is_empty());
        let package = tmp.path().join("export");
        crate::provenance::package(&output, &package).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        let resumed =
            SangerReviewSession::load(&package.join("consensus.sanger-review.json")).unwrap();
        assert_eq!(resumed.resolved_sequence().as_deref(), Some("AC"));
        assert!(resumed.working_draft().is_ok());
        assert!(
            crate::provenance::verify(&package.join("consensus.fa"))
                .unwrap()
                .is_empty()
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
        assert!(
            SangerReviewSession::load(&path)
                .unwrap()
                .working_draft()
                .is_err()
        );
        assert!(crate::provenance::package(&path, &tmp.path().join("bad-export")).is_err());
        assert!(!tmp.path().join("bad-export").exists());
        std::fs::remove_file(&trace).unwrap();
        assert!(
            SangerReviewSession::load(&path)
                .unwrap()
                .working_draft()
                .is_err()
        );
        std::fs::write(&trace, &original).unwrap();
        let conflict = inputs::conflicts(&trace).unwrap();
        assert!(!conflict.analyses.is_empty());
        inputs::resolve(&conflict, inputs::SavePolicy::PreserveCopy).unwrap();
        std::fs::write(&trace, b"ABIFnew user data").unwrap();
        let reopened = SangerReviewSession::load(&path).unwrap();
        let resolved = reopened.working_draft().unwrap();
        assert_eq!(std::fs::read(&resolved.inputs[0].path).unwrap(), original);
        assert_eq!(std::fs::read(&trace).unwrap(), b"ABIFnew user data");
        crate::provenance::package(&path, &tmp.path().join("preserved-export")).unwrap();
    }

    #[test]
    fn failed_save_keeps_recovery_and_unrelated_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut review, _) = draft(tmp.path());
        review.start_recovery().unwrap();
        edit(&mut review);
        let target = tmp.path().join("blocked.sanger-review.json");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("unrelated"), b"keep").unwrap();
        assert!(review.save(&target).is_err());
        assert_eq!(std::fs::read(target.join("unrelated")).unwrap(), b"keep");
        let recovered = SangerReviewSession::load(review.recovery.as_ref().unwrap()).unwrap();
        assert_eq!(recovered.events(), review.events());
        assert_eq!(recovered.decision(0), Some(&ReviewChoice::Edit('G')));
        let mut broken: serde_json::Value =
            serde_json::from_slice(&std::fs::read(review.recovery.as_ref().unwrap()).unwrap())
                .unwrap();
        broken["events"][0]["op"] = "damaged-journal".into();
        std::fs::write(
            review.recovery.as_ref().unwrap(),
            serde_json::to_vec(&broken).unwrap(),
        )
        .unwrap();
        assert!(SangerReviewSession::load(review.recovery.as_ref().unwrap()).is_err());
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
}
