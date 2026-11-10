//! Replay a project's log and check every step.
//!
//! Inputs come from the object store, or from the project if the file still
//! has the recorded hash. Each edit must give the same feature ids and
//! document state as recorded; each save must produce files with the
//! recorded hashes. Nothing in the project is written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use basecraft_protocol::{LogEntry, Op};

use crate::session::{DocId, Session, objects_dir};
use crate::{Result, log};

#[derive(Debug, Default)]
pub struct Report {
    pub sessions: usize,
    pub entries: usize,
    pub problems: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

pub fn replay(root: &Path) -> Result<Report> {
    let entries = log::read_all(root)?;
    let mut report = Report::default();
    let mut order: Vec<&str> = Vec::new();
    let mut by_session: HashMap<&str, Vec<&LogEntry>> = HashMap::new();
    for e in &entries {
        let list = by_session.entry(&e.session).or_default();
        if list.is_empty() {
            order.push(&e.session);
        }
        list.push(e);
    }
    for sid in order {
        report.sessions += 1;
        replay_session(root, &by_session[sid], &mut report)?;
    }
    Ok(report)
}

fn replay_session(root: &Path, entries: &[&LogEntry], report: &mut Report) -> Result<()> {
    let work = tempfile::tempdir()?;
    let mut session = Session::replay_session(work.path());
    let source_root = recorded_source_root(root)?;
    let mut docs: HashMap<&str, DocId> = HashMap::new();
    // Replay entry id -> logged entry id, to check undo and redo targets.
    let mut ids: HashMap<String, String> = HashMap::new();
    for e in entries {
        report.entries += 1;
        let mut problem = |msg: String| {
            report
                .problems
                .push(format!("{} {} {}: {msg}", e.id, e.op.name(), e.doc));
        };
        if let Op::CreateSequence { name, residues } = &e.op {
            docs.remove(e.doc.as_str());
            if !safe_key(&e.doc) {
                problem("unsafe document identity".into());
                continue;
            }
            match session.create_sequence_with_key(&e.doc, name, residues) {
                Ok(id) if session.state(id) == e.state => {
                    docs.insert(&e.doc, id);
                }
                Ok(_) => problem("created sequence state differs".into()),
                Err(error) => problem(error.to_string()),
            }
            continue;
        }
        if let Op::Open { path } = &e.op {
            // A refused baseline must not leave later edits targeting an older document.
            docs.remove(e.doc.as_str());
            if !safe_key(path) || !safe_key(&e.doc) || path != &e.doc {
                problem("unsafe or inconsistent document identity".into());
                continue;
            }
            match materialise(root, work.path(), &e.result.hashes) {
                Ok(()) => match session.open_replay_snapshot(&work.path().join(path)) {
                    Ok(id) => {
                        session.restore_replay_paths(id, work.path(), &source_root);
                        docs.insert(&e.doc, id);
                    }
                    Err(err) => problem(format!("can't open: {err}")),
                },
                Err(msg) => problem(msg),
            }
            continue;
        }
        let Some(&doc) = docs.get(e.doc.as_str()) else {
            problem("document wasn't opened".into());
            continue;
        };
        match &e.op {
            // Species metadata records the original folder as its relocation
            // base. Hash against that destination, not replay's disposable root.
            Op::Save { path } => match session.save_hashes_at(doc, &source_root.join(path)) {
                Ok(got) => {
                    let want: Vec<&String> = e.result.hashes.iter().map(|(_, h)| h).collect();
                    if got.iter().collect::<Vec<_>>() != want {
                        let legacy = session.legacy_delimitation_hashes(doc, Path::new(path))?;
                        if legacy
                            .as_ref()
                            .is_none_or(|hashes| hashes.iter().collect::<Vec<_>>() != want)
                        {
                            problem(format!("saved content differs: {got:?} vs {want:?}"));
                        }
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            Op::Extract {
                start,
                end,
                segments,
                reverse,
                ..
            } => match session.extract_hash(doc, (*start, *end), segments, *reverse) {
                Ok(got) => {
                    let want = e.result.hashes.first().map(|(_, h)| h.as_str());
                    if Some(got.as_str()) != want {
                        problem(format!("extracted content differs: {got} vs {want:?}"));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
            // External tools aren't re-run: their outputs are new files,
            // and opening one later checks it against its stored copy.
            // Exports are figures; drawing isn't part of replay.
            Op::ToolRun { .. } | Op::Export { .. } | Op::JobFinished { .. } => {}
            Op::Undo { of } | Op::Redo { of } => {
                let r = if matches!(e.op, Op::Undo { .. }) {
                    session.undo_single(doc)
                } else {
                    session.redo_single(doc)
                };
                match r {
                    Ok(replayed) if ids.get(&replayed) == Some(of) => {}
                    Ok(replayed) => {
                        problem(format!("targets {:?}, expected {of}", ids.get(&replayed)));
                    }
                    Err(err) => problem(err.to_string()),
                }
            }
            op => match session.apply_as(doc, op.clone(), e.actor.clone()) {
                Ok(out) => {
                    ids.insert(out.entry_id, e.id.clone());
                    if out.feature.map(|f| f.0) != e.result.feature {
                        problem(format!(
                            "feature id {:?}, expected {:?}",
                            out.feature, e.result.feature
                        ));
                    }
                }
                Err(err) => problem(err.to_string()),
            },
        }
        let state = session.state(doc);
        if state != e.state {
            report.problems.push(format!(
                "{} {}: state {state:?}, expected {:?}",
                e.id,
                e.op.name(),
                e.state
            ));
        }
    }
    Ok(())
}

fn recorded_source_root(root: &Path) -> Result<std::path::PathBuf> {
    let manifest = root.join("recovery.json");
    if !manifest.is_file() {
        return Ok(root.to_path_buf());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&crate::provenance::read_record(&manifest)?)?;
    if value["schema_version"] != 1 {
        return Err(crate::error::invalid("Unsupported recovery metadata"));
    }
    value["source_root"]
        .as_str()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| crate::error::invalid("Recovery has no original source folder"))
}

/// Put each recorded input into `work` under its project-relative path.
fn materialise(
    root: &Path,
    work: &Path,
    inputs: &[(String, String)],
) -> std::result::Result<(), String> {
    for (rel, hash) in inputs {
        if Path::new(rel)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe replay input path: {rel}"));
        }
        let Some(digest) = hash.strip_prefix("b3:") else {
            return Err("unsupported input hash".into());
        };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid input hash".into());
        }
        let stored = objects_dir(root).join(hash.trim_start_matches("b3:"));
        let original = root.join(rel);
        let source = if stored.exists() {
            stored
        } else if basecraft_io::cache::content_hash(&original).ok().as_deref() == Some(hash) {
            original
        } else if let Some(retained) =
            crate::analysis_store::retained_source(root, hash).map_err(|e| e.to_string())?
        {
            retained
        } else {
            return Err(format!("input {rel} changed and no stored copy exists"));
        };
        if basecraft_io::hash_file(&source).map_err(|e| e.to_string())? != *hash {
            return Err(format!("retained input {rel} hash differs"));
        }
        let mut dest = work.join(rel);
        if let Some((main, _)) = inputs.first()
            && crate::files::Format::from_path(Path::new(main)).ok()
                == Some(crate::files::Format::Delimitation)
            && Path::new(rel)
                .components()
                .any(|c| c.as_os_str() == "analyses-v1")
            && Path::new(rel).file_name()
                == crate::files::tree_sidecar(&work.join(main)).file_name()
        {
            // The replay namespace contains no live store bindings. Restore
            // the checked baseline as a standalone companion, not a partial
            // hidden namespace that could be mistaken for a real store.
            dest = crate::files::tree_sidecar(&work.join(main));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Always replace with an independent inode. The destination may already
        // alias a retained input (or contain bytes from an earlier baseline).
        // Copying onto it in place would truncate the immutable source.
        basecraft_io::write_atomic(&dest, |writer| {
            let mut input = std::fs::File::open(&source)?;
            std::io::copy(&mut input, writer)?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn safe_key(key: &str) -> bool {
    !key.is_empty()
        && Path::new(key)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Reconstruct unsaved documents into a new directory without modifying sources
/// or executing any tools. Strict reading rejects torn/corrupt journals; the
/// error names how many events [`recover_verified_prefix`] would keep.
/// This conservative wrapper drops its attempt on refusal. A returned check is
/// advisory only; continuations must use [`prepare_recovery`] on a live attempt.
pub fn recover(root: &Path, destination: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut attempt = prepare_recovery(root, destination, RecoveryMode::Strict)?;
    for document in 0..attempt.len() {
        attempt.advance(document, None)?;
    }
    Ok(attempt.publish()?.outputs)
}

/// Strict never falls back to a damaged journal's prefix implicitly.
#[derive(Clone, Copy, Debug)]
pub enum RecoveryMode {
    Strict,
    VerifiedPrefix,
}

pub fn prepare_recovery(
    root: &Path,
    destination: &Path,
    mode: RecoveryMode,
) -> Result<RecoveryAttempt> {
    recover_checked(root, destination)?;
    let (entries, damage) = match mode {
        RecoveryMode::Strict => (strict_entries(root)?, None),
        RecoveryMode::VerifiedPrefix => {
            let prefix = log::verified_session_prefix(root)?;
            (prefix.entries, prefix.problem)
        }
    };
    recover_entries(root, destination, entries, damage)
}

fn strict_entries(root: &Path) -> Result<Vec<LogEntry>> {
    log::read_all(root).map_err(|error| {
        let kept = log::verified_session_prefix(root)
            .ok()
            .filter(|prefix| prefix.problem.is_some());
        match kept {
            Some(prefix) => crate::error::invalid(format!(
                "the recovery journal is damaged ({}); nothing was recovered. The {} verified journal events before the damage can be recovered explicitly with `basecraft recover --verified-prefix`; the journal is left unchanged",
                prefix.problem.unwrap_or_default(),
                prefix.entries.len()
            )),
            None => error,
        }
    })
}

/// Explicit salvage after a crash: reconstruct only the journal's verified
/// prefix, stopping at the first torn or corrupt line. Later events are never
/// skipped over, and the damaged journal stays as it is for inspection.
pub fn recover_verified_prefix(
    root: &Path,
    destination: &Path,
) -> Result<(Vec<std::path::PathBuf>, Option<String>)> {
    let mut attempt = prepare_recovery(root, destination, RecoveryMode::VerifiedPrefix)?;
    for document in 0..attempt.len() {
        attempt.advance(document, None)?;
    }
    let completed = attempt.publish()?;
    Ok((completed.outputs, completed.damage))
}

fn recover_checked(root: &Path, destination: &Path) -> Result<()> {
    if root.join("recovery-integrity-error.json").exists() {
        return Err(crate::error::invalid(
            "journal durability was uncertain; inspect the preserved verified prefix before recovery",
        ));
    }
    require_absent(destination)
}

fn require_absent(destination: &Path) -> Result<()> {
    match std::fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err(crate::error::invalid("recovery destination already exists")),
    }
}

fn recover_entries(
    root: &Path,
    destination: &Path,
    entries: Vec<LogEntry>,
    damage: Option<String>,
) -> Result<RecoveryAttempt> {
    if entries.is_empty() {
        return Err(crate::error::invalid(
            "recovery contains no verified events",
        ));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    let parent_identity = directory_identity(&parent)?;
    let name = destination
        .file_name()
        .ok_or_else(|| crate::error::invalid("recovery destination needs a basename"))?;
    let destination = parent.join(name);
    require_absent(&destination)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-recover-")
        .tempdir_in(&parent)?;
    let mut session = Session::unlogged(staging.path());
    let source_root = recorded_source_root(root)?;
    let mut documents = std::collections::BTreeMap::<String, DocId>::new();
    for entry in entries {
        if let Op::CreateSequence { name, residues } = &entry.op {
            if !safe_key(&entry.doc) {
                return Err(crate::error::invalid("unsafe recovery document identity"));
            }
            let doc = session.create_sequence_with_key(&entry.doc, name, residues)?;
            if session.state(doc) != entry.state {
                return Err(crate::error::invalid("created sequence state differs"));
            }
            documents.insert(entry.doc.clone(), doc);
            continue;
        }
        if let Op::Open { path } = &entry.op {
            if !safe_key(path) || !safe_key(&entry.doc) || path != &entry.doc {
                return Err(crate::error::invalid(
                    "unsafe or inconsistent recovery document identity",
                ));
            }
            materialise(root, staging.path(), &entry.result.hashes)
                .map_err(crate::error::invalid)?;
            let doc = session.open(&staging.path().join(path))?;
            session.restore_replay_paths(doc, staging.path(), &source_root);
            documents.insert(entry.doc.clone(), doc);
            continue;
        }
        let doc = *documents
            .get(&entry.doc)
            .ok_or_else(|| crate::error::invalid("recovery event precedes its baseline"))?;
        match entry.op {
            Op::Save { .. }
            | Op::ToolRun { .. }
            | Op::JobFinished { .. }
            | Op::Export { .. }
            | Op::Extract { .. } => {}
            Op::Undo { .. } => {
                session.undo_single(doc)?;
            }
            Op::Redo { .. } => {
                session.redo_single(doc)?;
            }
            operation => {
                session.apply_as(doc, operation, entry.actor)?;
            }
        }
        if session.state(doc) != entry.state {
            return Err(crate::error::invalid(format!(
                "recovery state differs at {}",
                entry.id
            )));
        }
    }
    session.recovery_placement(staging.path().to_path_buf(), destination.clone());
    let documents = documents
        .into_iter()
        .map(|(relative, id)| RecoveryDocument {
            relative,
            id,
            saved: None,
        })
        .collect();
    let artifacts = artifacts(staging.path())?;
    Ok(RecoveryAttempt {
        session,
        staging: Some(staging),
        destination,
        parent_identity,
        documents,
        artifacts,
        warnings: Vec::new(),
        damage,
        published: false,
        #[cfg(test)]
        before_sync: None,
        #[cfg(test)]
        before_promote: None,
        #[cfg(test)]
        after_promote: None,
    })
}

/// Lifetime owner only. Session Slots retain all receipt authority. Dropping or
/// cancelling removes only this attempt's disposable stage; originals survive.
/// A failed pre-publication call leaves the attempt available for a fresh check.
pub struct RecoveryAttempt {
    session: Session,
    staging: Option<tempfile::TempDir>,
    destination: PathBuf,
    parent_identity: (u64, u64),
    documents: Vec<RecoveryDocument>,
    artifacts: Vec<ArtifactObservation>,
    warnings: Vec<crate::session::SaveWarning>,
    damage: Option<String>,
    published: bool,
    #[cfg(test)]
    before_sync: Option<PublicationBarrier>,
    #[cfg(test)]
    before_promote: Option<PublicationBarrier>,
    #[cfg(test)]
    after_promote: Option<PublicationBarrier>,
}

#[cfg(test)]
type PublicationBarrier = Box<dyn FnOnce(&Path)>;

struct RecoveryDocument {
    relative: String,
    id: DocId,
    // Expected outcome, never authority. This observation has no published token.
    saved: Option<crate::session::SaveCheck>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RecoveryCompletion {
    pub destination: PathBuf,
    pub outputs: Vec<PathBuf>,
    pub warnings: Vec<crate::session::SaveWarning>,
    pub damage: Option<String>,
    pub registered_stores: Vec<PathBuf>,
    pub evidence_actions: Vec<crate::session::EvidenceAction>,
}

/// Rename completed, but a later independent sync/registration did not. The
/// final files remain. This is terminal, not permission to retry into them.
#[derive(Debug, thiserror::Error)]
#[error("recovery was published at {destination}, but completion failed: {cause}", destination = .completion.destination.display())]
pub struct PublishedWithFailure {
    pub completion: RecoveryCompletion,
    #[source]
    pub cause: crate::Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArtifactObservation {
    path: PathBuf,
    identity: (u64, u64),
    revision: Option<(String, basecraft_protocol::provenance::FileStamp)>,
}

pub(crate) fn directory_identity(path: &Path) -> Result<(u64, u64)> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(crate::error::invalid(
            "recovery parent is not an ordinary directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        Err(crate::error::invalid(
            "recovery directory identity is unsupported on this platform",
        ))
    }
}

/// Enumerate only the owned stage, never external stores or arbitrary trees.
/// Every file and directory must remain ordinary; equal bytes do not merge inodes.
fn artifacts(root: &Path) -> Result<Vec<ArtifactObservation>> {
    fn visit(root: &Path, path: &Path, result: &mut Vec<ArtifactObservation>) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        let (identity, revision) = if metadata.is_dir() && !metadata.file_type().is_symlink() {
            (directory_identity(path)?, None)
        } else if metadata.is_file() && !metadata.file_type().is_symlink() {
            (
                basecraft_platform::files::identity(path)?,
                Some((
                    basecraft_io::hash_file(path)?,
                    crate::input_evidence::stamp(path)?,
                )),
            )
        } else {
            return Err(crate::error::invalid("unsafe recovery artifact"));
        };
        result.push(ArtifactObservation {
            path: path
                .strip_prefix(root)
                .map_err(|_| crate::error::invalid("artifact outside recovery stage"))?
                .into(),
            identity,
            revision,
        });
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                visit(root, &entry?.path(), result)?;
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, root, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

impl RecoveryAttempt {
    pub fn len(&self) -> usize {
        self.documents.len()
    }
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn damage(&self) -> Option<&str> {
        self.damage.as_deref()
    }
    pub fn warnings(&self) -> &[crate::session::SaveWarning] {
        &self.warnings
    }
    pub fn evidence_actions(&self) -> Result<Vec<crate::session::EvidenceAction>> {
        self.session.recovery_actions()
    }
    pub fn document_for_check(&self, check: &crate::session::SaveCheck) -> Option<usize> {
        self.documents
            .iter()
            .position(|doc| doc.id.0 == check.document())
    }
    pub fn cancel(self) {
        drop(self);
    }

    fn document(&self, document: usize) -> Result<&RecoveryDocument> {
        self.documents
            .get(document)
            .ok_or_else(|| crate::error::invalid("unknown recovery document"))
    }

    fn validate(&self) -> Result<&Path> {
        if self.published {
            return Err(crate::error::invalid("recovery has already been published"));
        }
        let stage = self
            .staging
            .as_ref()
            .ok_or_else(|| crate::error::invalid("recovery is cancelled"))?
            .path();
        let parent = self
            .destination
            .parent()
            .ok_or_else(|| crate::error::invalid("recovery parent missing"))?;
        if directory_identity(parent)? != self.parent_identity {
            return Err(crate::error::invalid(
                "recovery destination parent was replaced",
            ));
        }
        require_absent(&self.destination)?;
        if artifacts(stage)? != self.artifacts {
            return Err(crate::error::invalid(
                "recovery staged artifacts changed; reconstruct a new attempt",
            ));
        }
        Ok(stage)
    }

    /// Fresh advisory check for just one document. Calls invalidate older tokens
    /// for that Slot. The final destination can never be changed on this attempt.
    pub fn check(&self, document: usize) -> Result<crate::session::SaveCheck> {
        self.validate()?;
        let doc = self.document(document)?;
        self.session.recovery_check(doc.id, doc.saved.is_some())
    }

    pub fn decide(
        &self,
        document: usize,
        token: &str,
        known: crate::input_evidence::SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<crate::session::SaveDecision> {
        self.validate()?;
        let doc = self.document(document)?;
        match self
            .session
            .decide_save(doc.id, None, token, known, acknowledge_incomplete)
        {
            Err(crate::Error::SaveDecisionStale(_)) => Err(crate::Error::SaveDecisionStale(
                Box::new(self.session.recovery_check(doc.id, doc.saved.is_some())?),
            )),
            other => other,
        }
    }

    /// Stage one document, or reauthorize its finalization if already saved.
    /// Does not publish the destination and never resaves a completed document.
    pub fn advance(
        &mut self,
        document: usize,
        decision: Option<crate::session::SaveDecision>,
    ) -> Result<()> {
        if let Err(error) = self
            .validate()
            .and_then(|_| self.document(document).map(|_| ()))
        {
            if let Some(decision) = &decision {
                self.session.discard_recovery_decision(decision)?;
            }
            return Err(error);
        }
        let doc = self.document(document)?;
        let id = doc.id;
        let _target = match crate::save_guard::lock(&self.destination) {
            Ok(lock) => lock,
            Err(error) => {
                if let Some(decision) = &decision {
                    self.session.discard_recovery_decision(decision)?;
                }
                return Err(error);
            }
        };
        let finalizing = doc.saved.is_some();
        let observed = if let Some(expected) = &doc.saved {
            let _evidence = crate::input_evidence::Mutation::begin()?;
            self.session.recovery_finalize(id, expected, decision)?
        } else {
            let outcome = match decision {
                Some(decision) => self.session.save_with_decision(
                    id,
                    Some(&self.session_path(document)?),
                    decision,
                ),
                None => self.session.save_recovery(id),
            };
            // Failed writes can leave partial owned output; Session retains its
            // existing acceptance rules. This is an observation, never consent.
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.artifacts = artifacts(
                        self.staging
                            .as_ref()
                            .ok_or_else(|| crate::error::invalid("recovery stage missing"))?
                            .path(),
                    )?;
                    return Err(error);
                }
            };
            self.warnings.extend(outcome.warnings);
            outcome
                .recovery_observation
                .ok_or_else(|| crate::error::invalid("recovery write observation missing"))?
        };
        if finalizing && !observed.complete() {
            self.warnings
                .push(crate::session::SaveWarning::EvidenceNotChecked {
                    issues: observed.issues(),
                });
        }
        self.documents[document].saved = Some(observed);
        self.artifacts = artifacts(
            self.staging
                .as_ref()
                .ok_or_else(|| crate::error::invalid("recovery stage missing"))?
                .path(),
        )?;
        Ok(())
    }

    fn session_path(&self, document: usize) -> Result<PathBuf> {
        Ok(self
            .staging
            .as_ref()
            .ok_or_else(|| crate::error::invalid("recovery stage missing"))?
            .path()
            .join(&self.document(document)?.relative))
    }

    pub fn publish(&mut self) -> Result<RecoveryCompletion> {
        self.validate()?;
        let _target = crate::save_guard::lock(&self.destination)?;
        let evidence = crate::input_evidence::Mutation::begin()?;
        for doc in &self.documents {
            let expected = doc
                .saved
                .as_ref()
                .ok_or_else(|| crate::error::invalid("recovery documents still need saving"))?;
            self.session.recovery_finalize(doc.id, expected, None)?;
        }
        let stage = self.validate()?.to_path_buf();
        let stores = self.session.recovery_stores()?;
        let evidence_actions = self.session.recovery_actions()?;
        #[cfg(test)]
        if let Some(barrier) = self.before_sync.take() {
            barrier(&stage);
        }
        for artifact in self.artifacts.iter().rev() {
            let path = stage.join(&artifact.path);
            if artifact.revision.is_some() {
                basecraft_platform::files::sync_file(&path)?;
            } else {
                basecraft_platform::files::sync_directory(&path)?;
            }
        }
        self.validate()?;
        #[cfg(test)]
        if let Some(barrier) = self.before_promote.take() {
            barrier(&self.destination);
        }
        basecraft_platform::atomic::rename_noreplace(&stage, &self.destination)?;
        // Disarm immediately: post-promotion errors must never imply rollback.
        if let Some(staging) = self.staging.take() {
            let _ = staging.keep();
        }
        self.published = true;
        #[cfg(test)]
        if let Some(barrier) = self.after_promote.take() {
            barrier(&self.destination);
        }
        let mut completion = RecoveryCompletion {
            destination: self.destination.clone(),
            outputs: self
                .documents
                .iter()
                .map(|d| self.destination.join(&d.relative))
                .collect(),
            warnings: self.warnings.clone(),
            damage: self.damage.clone(),
            registered_stores: Vec::new(),
            evidence_actions,
        };
        let finish = (|| -> Result<()> {
            basecraft_platform::files::sync_directory(
                self.destination
                    .parent()
                    .ok_or_else(|| crate::error::invalid("recovery parent missing"))?,
            )?;
            for run in stores {
                let final_run = run.relocated(&stage, &self.destination)?;
                evidence.register(&final_run)?;
                let store = final_run
                    .directory()
                    .parent()
                    .and_then(Path::parent)
                    .ok_or_else(|| crate::error::invalid("recovery run has no store"))?
                    .to_path_buf();
                if !completion.registered_stores.contains(&store) {
                    completion.registered_stores.push(store);
                }
            }
            Ok(())
        })();
        match finish {
            Ok(()) => Ok(completion),
            Err(cause) => Err(crate::Error::PublishedWithFailure(Box::new(
                PublishedWithFailure { completion, cause },
            ))),
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/isolated_profile.rs"]
mod isolated_recovery_profile;

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(root: &Path, spart: bool) -> (Session, RecoveryAttempt) {
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        let path = source.join(if spart { "a.spart" } else { "a.fasta" });
        let bytes = if spart {
            include_str!("../../../testdata/fixtures/delimitation/manual.spart")
        } else {
            ">gene\nACGT\n"
        };
        std::fs::write(&path, bytes).unwrap();
        let mut session = Session::recoverable(&source).unwrap();
        let doc = session.open(&path).unwrap();
        let op = if spart {
            Op::DelimitationEdit {
                edit: Box::new(basecraft_protocol::delimitation::DelimitationEdit::Remove {
                    hypotheses: ["partition-3".into()].into(),
                }),
            }
        } else {
            Op::SeqReplace {
                start: 1,
                end: 2,
                residues: "T".into(),
            }
        };
        session.apply(doc, op).unwrap();
        session.flush().unwrap();
        let mut attempt = prepare_recovery(
            session.recovery_root(),
            &root.join("final"),
            RecoveryMode::Strict,
        )
        .unwrap();
        attempt.advance(0, None).unwrap();
        (session, attempt)
    }

    #[test]
    fn final_noreplace_barrier_preserves_competitors() {
        let Some(root) = isolated_recovery_profile::enter(
            "replay::tests::final_noreplace_barrier_preserves_competitors",
        ) else {
            return;
        };
        for kind in 0..4 {
            let case = root.join(kind.to_string());
            std::fs::create_dir(&case).unwrap();
            let (_session, mut attempt) = prepared(&case, false);
            attempt.before_promote = Some(Box::new(move |destination| match kind {
                0 => std::fs::write(destination, "competitor").unwrap(),
                1 => std::fs::create_dir(destination).unwrap(),
                2 => {
                    std::fs::create_dir(destination).unwrap();
                    std::fs::write(destination.join("owned"), "competitor").unwrap();
                }
                _ => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink("missing", destination).unwrap();
                }
            }));
            assert!(matches!(attempt.publish(), Err(crate::Error::Io(_))));
            assert!(!attempt.published);
            assert!(std::fs::symlink_metadata(case.join("final")).is_ok());
        }
    }

    #[test]
    fn post_promotion_registration_failure_retains_final_files() {
        let Some(root) = isolated_recovery_profile::enter(
            "replay::tests::post_promotion_registration_failure_retains_final_files",
        ) else {
            return;
        };
        let (_session, mut attempt) = prepared(&root, true);
        let registry = basecraft_platform::paths::AppPaths::discover()
            .unwrap()
            .state
            .join("analysis-stores.json");
        assert!(!registry.exists());
        attempt.after_promote = Some(Box::new(move |_| {
            std::fs::create_dir(&registry).unwrap();
        }));
        let crate::Error::PublishedWithFailure(failure) = attempt.publish().unwrap_err() else {
            panic!("expected published failure")
        };
        assert!(matches!(failure.cause, crate::Error::Io(_)));
        assert!(failure.completion.registered_stores.is_empty());
        assert_eq!(failure.completion.outputs.len(), 1);
        drop(attempt);
        let work = crate::files::load_delimitation(&failure.completion.outputs[0]).unwrap();
        assert_eq!(work.hypotheses.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn sync_permission_failures_distinguish_before_and_after_promotion() {
        use std::os::unix::fs::PermissionsExt;
        let Some(root) = isolated_recovery_profile::enter(
            "replay::tests::sync_permission_failures_distinguish_before_and_after_promotion",
        ) else {
            return;
        };
        let pre = root.join("pre");
        std::fs::create_dir(&pre).unwrap();
        let (_session, mut attempt) = prepared(&pre, false);
        let file = attempt.check(0).unwrap().target().to_path_buf();
        attempt.before_sync = Some(Box::new(|stage| {
            std::fs::set_permissions(stage.join("a.fasta"), std::fs::Permissions::from_mode(0))
                .unwrap()
        }));
        assert!(matches!(attempt.publish(), Err(crate::Error::Io(_))));
        assert!(!pre.join("final").exists());
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(attempt);
        let post = root.join("post");
        std::fs::create_dir(&post).unwrap();
        let (_session, mut attempt) = prepared(&post, false);
        attempt.after_promote = Some(Box::new(|destination| {
            std::fs::set_permissions(
                destination.parent().unwrap(),
                std::fs::Permissions::from_mode(0),
            )
            .unwrap()
        }));
        let failure = attempt.publish().unwrap_err();
        std::fs::set_permissions(&post, std::fs::Permissions::from_mode(0o700)).unwrap();
        let crate::Error::PublishedWithFailure(failure) = failure else {
            panic!("expected published failure")
        };
        assert!(matches!(failure.cause, crate::Error::Io(_)));
        drop(attempt);
        assert_eq!(
            std::fs::read(&failure.completion.outputs[0]).unwrap(),
            b">gene\nATGT\n"
        );
    }

    #[test]
    fn materialise_replaces_aliases_and_different_baselines_without_touching_sources() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(objects_dir(root.path())).unwrap();
        for retained in [false, true] {
            let bytes = if retained { b"retained" } else { b"original" };
            let hash = basecraft_io::hash_bytes(bytes);
            let source = if retained {
                objects_dir(root.path()).join(hash.strip_prefix("b3:").unwrap())
            } else {
                root.path().join("a.fa")
            };
            std::fs::write(&source, bytes).unwrap();
            let dest = work.path().join("a.fa");
            if dest.exists() {
                std::fs::remove_file(&dest).unwrap();
            }
            std::fs::hard_link(&source, &dest).unwrap();
            let inputs = vec![("a.fa".into(), hash.clone())];
            materialise(root.path(), work.path(), &inputs).unwrap();
            assert_eq!(std::fs::read(&dest).unwrap(), bytes);
            // Editing scratch must also be independent of the retained inode.
            std::fs::write(&dest, "scratch edit").unwrap();
            assert_eq!(basecraft_io::hash_file(&source).unwrap(), hash);
            materialise(root.path(), work.path(), &inputs).unwrap();
            assert_eq!(std::fs::read(&dest).unwrap(), bytes);
            assert_eq!(basecraft_io::hash_file(&source).unwrap(), hash);
        }
    }

    #[test]
    fn materialise_failure_preserves_destination_and_cleans_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(objects_dir(root.path())).unwrap();
        let hash = basecraft_io::hash_bytes(b"expected");
        let object = objects_dir(root.path()).join(hash.strip_prefix("b3:").unwrap());
        std::fs::write(&object, "corrupt").unwrap();
        let inputs = vec![("a.fa".into(), hash)];
        let dest = work.path().join("a.fa");
        std::fs::write(&dest, "previous").unwrap();
        assert!(materialise(root.path(), work.path(), &inputs).is_err());
        assert_eq!(std::fs::read(&object).unwrap(), b"corrupt");
        assert_eq!(std::fs::read(&dest).unwrap(), b"previous");
        std::fs::remove_file(&object).unwrap();
        assert!(materialise(root.path(), work.path(), &inputs).is_err());
        std::fs::write(root.path().join("a.fa"), "changed").unwrap();
        assert!(materialise(root.path(), work.path(), &inputs).is_err());
        // Force atomic persistence failure after the temporary copy is written.
        std::fs::write(root.path().join("a.fa"), "expected").unwrap();
        std::fs::remove_file(&dest).unwrap();
        std::fs::create_dir(&dest).unwrap();
        assert!(materialise(root.path(), work.path(), &inputs).is_err());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(root.path().join("a.fa")).unwrap(),
            b"expected"
        );
        for rel in ["../escape", "/absolute"] {
            assert!(
                materialise(
                    root.path(),
                    work.path(),
                    &[(rel.into(), inputs[0].1.clone())]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn refused_baselines_invalidate_previous_mapping() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let mut session = Session::new(root.path());
        let doc = session.open(&path).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                },
            )
            .unwrap();
        session.flush().unwrap();
        let entries = log::read_all(root.path()).unwrap();
        let baseline = entries
            .iter()
            .find(|e| matches!(e.op, Op::Open { .. }))
            .unwrap();
        let edit = entries
            .iter()
            .find(|e| matches!(e.op, Op::SeqReplace { .. }))
            .unwrap();
        let mut attempts = Vec::new();
        let mut missing = baseline.clone();
        missing.result.hashes[0].1 = basecraft_io::hash_bytes(b"missing");
        attempts.push(missing);
        let mut unsafe_open = baseline.clone();
        unsafe_open.op = Op::Open {
            path: "../escape".into(),
        };
        attempts.push(unsafe_open);
        let mut invalid_create = baseline.clone();
        invalid_create.op = Op::CreateSequence {
            name: "invalid name".into(),
            residues: "!".into(),
        };
        attempts.push(invalid_create);
        let mut divergent_create = baseline.clone();
        divergent_create.op = Op::CreateSequence {
            name: "new".into(),
            residues: "ACGTACGT".into(),
        };
        attempts.push(divergent_create);
        for attempt in attempts {
            let mut report = Report::default();
            replay_session(root.path(), &[baseline, &attempt, edit], &mut report).unwrap();
            assert_eq!(report.problems.len(), 2, "{report:#?}");
            assert!(
                report.problems[1].contains("document wasn't opened"),
                "{report:#?}"
            );
            let destination = root.path().join("recovered");
            assert!(
                recover_entries(
                    root.path(),
                    &destination,
                    vec![baseline.clone(), attempt, edit.clone()],
                    None
                )
                .is_err()
            );
            assert!(!destination.exists());
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".basecraft-recover-")
            }));
        }
    }
}
