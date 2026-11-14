//! Replay a project's log and check every step.
//!
//! Inputs come from the object store, or from the project if the file still
//! has the recorded hash. Each edit must give the same feature ids and
//! document state as recorded; each save must produce files with the
//! recorded hashes. Nothing in the project is written.

use crate::error::{damaged, limit};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use basecraft_platform::files::{ObjectIdentity, OrdinaryKind, object_identity};
use basecraft_protocol::{LogEntry, Op, ops::ProposalDecision};

use crate::session::{DocId, Session, objects_dir};
use crate::{Error, Result, log};

#[derive(Debug, Default)]
pub struct ReplayReport {
    pub sessions: usize,
    pub entries: usize,
    /// Entries that could not be checked because their original was replaced
    /// and no later baseline of that document was available yet.
    pub unchecked: usize,
    pub problems: Vec<String>,
}

impl ReplayReport {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

pub fn replay(root: &Path) -> Result<ReplayReport> {
    let written = log::read_all_written(root)?;
    // Saves are written again as the build that logged them wrote them.
    let writers: HashMap<&str, &str> = written
        .iter()
        .map(|(e, writer)| (e.id.as_str(), writer.as_str()))
        .collect();
    let mut report = ReplayReport::default();
    let mut order: Vec<&str> = Vec::new();
    let mut by_session: HashMap<&str, Vec<&LogEntry>> = HashMap::new();
    for (e, _) in &written {
        let list = by_session.entry(&e.session).or_default();
        if list.is_empty() {
            order.push(&e.session);
        }
        list.push(e);
    }
    for sid in order {
        report.sessions += 1;
        replay_session(root, &by_session[sid], &writers, &mut report)?;
    }
    Ok(report)
}

fn replay_session(
    root: &Path,
    entries: &[&LogEntry],
    writers: &HashMap<&str, &str>,
    report: &mut ReplayReport,
) -> Result<()> {
    let work = tempfile::tempdir()?;
    let mut session = Session::replay_session(work.path());
    let source_root = recorded_source_root(root, entries.iter().copied());
    let mut docs: HashMap<&str, DocId> = HashMap::new();
    // Documents whose original was replaced: their entries are not checkable
    // until a later Save or checkpoint whose inputs are still available.
    let mut replaced: std::collections::HashSet<&str> = std::collections::HashSet::new();
    // Documents restarted at such a baseline: undo cannot reach before it.
    let mut rebased: std::collections::HashSet<&str> = std::collections::HashSet::new();
    // Replay entry id -> logged entry id, to check undo and redo targets.
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut proposals = Proposals::default();
    for &e in entries {
        report.entries += 1;
        if let Some(msg) = proposals.check(e) {
            report
                .problems
                .push(format!("{} {} {}: {msg}", e.id, e.op.name(), e.doc));
        }
        let mut problem = |msg: String| {
            report
                .problems
                .push(format!("{} {} {}: {msg}", e.id, e.op.name(), e.doc));
        };
        if matches!(
            e.op,
            Op::CreateSangerReview { .. } | Op::CreateSequence { .. }
        ) {
            replaced.remove(e.doc.as_str());
            rebased.remove(e.doc.as_str());
        }
        if let Op::CreateSangerReview { review } = &e.op {
            docs.remove(e.doc.as_str());
            if !safe_key(&e.doc) {
                problem("unsafe document identity".into());
                continue;
            }
            match session.create_sanger_review_with_key(&e.doc, &review_record_at(root, review)) {
                Ok(id) if session.state(id)?.matches_recorded(&e.state) => {
                    session.restore_replay_paths(id, work.path(), &source_root);
                    docs.insert(&e.doc, id);
                }
                Ok(_) => problem("created review state differs".into()),
                Err(error) => problem(error.to_string()),
            }
            continue;
        }
        if let Op::CreateSequence { name, residues } = &e.op {
            docs.remove(e.doc.as_str());
            if !safe_key(&e.doc) {
                problem("unsafe document identity".into());
                continue;
            }
            match session.create_sequence_with_key(&e.doc, name, residues) {
                Ok(id) if session.state(id)?.matches_recorded(&e.state) => {
                    docs.insert(&e.doc, id);
                }
                Ok(_) => problem("created sequence state differs".into()),
                Err(error) => problem(error.to_string()),
            }
            continue;
        }
        let checkpoint = e.source.as_ref().is_some_and(|source| source.checkpoint);
        let baseline = match &e.op {
            Op::Open { path } if !checkpoint || replaced.contains(e.doc.as_str()) => Some(path),
            // A journal cut back after a save starts the document there.
            Op::Save { path }
                if replaced.contains(e.doc.as_str()) || !docs.contains_key(e.doc.as_str()) =>
            {
                Some(path)
            }
            _ => None,
        };
        if let Some(path) = baseline {
            let restart = !matches!(e.op, Op::Open { .. }) || checkpoint;
            // A refused baseline must not leave later edits targeting an older document.
            docs.remove(e.doc.as_str());
            if !safe_key(path) || !safe_key(&e.doc) || (!restart && path != &e.doc) {
                problem("unsafe or inconsistent document identity".into());
                continue;
            }
            match materialise(root, &source_root, work.path(), &e.result.hashes) {
                Ok(()) => match session.open(&work.path().join(path)) {
                    Ok(id) => {
                        session.restore_replay_paths(id, work.path(), &source_root);
                        if restart {
                            if let Some(source) = &e.source
                                && let Err(err) = session.restore_recovery_ids(id, source)
                            {
                                problem(err.to_string());
                                continue;
                            }
                            if !session.state(id)?.matches_recorded(&e.state) {
                                problem("baseline state differs".into());
                                continue;
                            }
                            rebased.insert(&e.doc);
                        } else {
                            rebased.remove(e.doc.as_str());
                        }
                        replaced.remove(e.doc.as_str());
                        docs.insert(&e.doc, id);
                    }
                    Err(err) => problem(format!("can't open: {err}")),
                },
                Err(InputFailure::Replaced(_)) => {
                    replaced.insert(&e.doc);
                    report.unchecked += 1;
                }
                Err(InputFailure::Invalid(msg)) => problem(msg),
            }
            continue;
        }
        if replaced.contains(e.doc.as_str()) {
            report.unchecked += 1;
            continue;
        }
        if let Op::Open { .. } = &e.op {
            if docs.get(e.doc.as_str()).is_none_or(|doc| {
                !session
                    .state(*doc)
                    .is_ok_and(|state| state.matches_recorded(&e.state))
            }) {
                let replayed = docs.get(e.doc.as_str()).map(|doc| session.state(*doc));
                problem(format!(
                    "checkpoint state differs: recorded {:?}, replayed {replayed:?}",
                    e.state
                ));
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
            Op::Save { path, .. } => match basecraft_protocol::records::writing_as(
                writers
                    .get(e.id.as_str())
                    .copied()
                    .unwrap_or(basecraft_protocol::VERSION),
                || session.save_hashes_at(doc, &source_root.join(path)),
            ) {
                Ok(got) => {
                    let want: Vec<&String> = e.result.hashes.iter().map(|(_, h)| h).collect();
                    if got.iter().collect::<Vec<_>>() != want {
                        problem(format!("saved content differs: {got:?} vs {want:?}"));
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
            // Proposal entries change nothing; `Proposals` checks them.
            Op::ToolRun { .. }
            | Op::Export { .. }
            | Op::JobFinished { .. }
            | Op::ProposalOffer { .. }
            | Op::ProposalDecide { .. } => {}
            Op::Undo { of } | Op::Redo { of } => {
                let r = if matches!(e.op, Op::Undo { .. }) {
                    session.undo_single(doc)
                } else {
                    session.redo_single(doc)
                };
                match r {
                    Ok(replayed) if ids.get(&replayed) == Some(of) => {}
                    // Undo or redo reaching before a restart is not checkable.
                    _ if rebased.contains(e.doc.as_str()) => {
                        docs.remove(e.doc.as_str());
                        replaced.insert(&e.doc);
                        report.unchecked += 1;
                        continue;
                    }
                    Ok(replayed) => {
                        problem(format!("targets {:?}, expected {of}", ids.get(&replayed)));
                    }
                    Err(err) => problem(err.to_string()),
                }
            }
            op => {
                match session.apply_as(doc, sanger_record_at(root, op.clone()), e.actor.clone()) {
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
                }
            }
        }
        let state = session.state(doc)?;
        if !state.matches_recorded(&e.state) {
            report.problems.push(format!(
                "{} {}: state {state:?}, expected {:?}",
                e.id,
                e.op.name(),
                e.state
            ));
        }
    }
    for proposal in proposals.undecided() {
        report
            .problems
            .push(format!("proposal {proposal} was offered and never decided"));
    }
    Ok(())
}

/// The entries of one session so far and where each offer sits, to check
/// that every decision closes its offer: the writer appends the offer, the
/// edits it applied and the decision as one journal batch.
#[derive(Default)]
struct Proposals<'a> {
    log: Vec<&'a LogEntry>,
    /// Proposal id -> position of its offer in `log`.
    offers: HashMap<&'a str, usize>,
    decided: std::collections::HashSet<&'a str>,
}

impl<'a> Proposals<'a> {
    /// Note entry `e`; why it is inconsistent, if it is.
    fn check(&mut self, e: &'a LogEntry) -> Option<String> {
        let problem = match &e.op {
            Op::ProposalOffer { proposal, .. } => self
                .offers
                .insert(proposal, self.log.len())
                .map(|_| format!("proposal {proposal} is offered twice")),
            Op::ProposalDecide {
                proposal,
                outcome,
                applied,
                ..
            } => self.decide(e, proposal, *outcome, applied),
            _ => None,
        };
        self.log.push(e);
        problem
    }

    fn decide(
        &mut self,
        e: &LogEntry,
        proposal: &'a str,
        outcome: ProposalDecision,
        applied: &[String],
    ) -> Option<String> {
        let Some(&at) = self.offers.get(proposal) else {
            return Some(format!("proposal {proposal} wasn't offered before"));
        };
        let offer = self.log[at];
        if offer.doc != e.doc {
            return Some(format!("proposal {proposal} was offered for {}", offer.doc));
        }
        if !self.decided.insert(proposal) {
            return Some(format!("proposal {proposal} is decided twice"));
        }
        let between = &self.log[at + 1..];
        if !between
            .iter()
            .map(|edit| edit.id.as_str())
            .eq(applied.iter().map(String::as_str))
        {
            return Some(
                "applied isn't exactly the edits between the offer and the decision".into(),
            );
        }
        if let Some(edit) = between
            .iter()
            .find(|edit| edit.doc != offer.doc || edit.actor != offer.actor)
        {
            return Some(format!(
                "applied edit {} isn't the offerer's edit to {}",
                edit.id, offer.doc
            ));
        }
        let Op::ProposalOffer { operations, .. } = &offer.op else {
            return None;
        };
        let as_offered = operations
            .iter()
            .map(Op::name)
            .eq(between.iter().map(|edit| edit.op.name()));
        match outcome {
            ProposalDecision::Rejected if !applied.is_empty() => {
                Some("a rejection lists applied edits".into())
            }
            ProposalDecision::Accepted if !as_offered => {
                Some("accepted, but the edits applied aren't the edits offered".into())
            }
            ProposalDecision::Modified if applied.is_empty() => {
                Some("modified, but nothing applied".into())
            }
            _ => None,
        }
    }

    fn undecided(&self) -> impl Iterator<Item = &&'a str> {
        self.offers
            .keys()
            .filter(|proposal| !self.decided.contains(*proposal))
    }
}

fn recorded_source_root<'a>(
    root: &Path,
    mut entries: impl Iterator<Item = &'a LogEntry>,
) -> PathBuf {
    // A journal of created documents only names no original folder.
    entries
        .find_map(|entry| entry.source.as_ref())
        .map_or_else(|| root.to_path_buf(), |source| root.join(&source.root))
}

/// Why recorded inputs could not be put in place.
#[derive(Debug)]
enum InputFailure {
    /// No unchanged copy of an input exists any more.
    Replaced(String),
    Invalid(String),
}

impl std::fmt::Display for InputFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::Replaced(message) | Self::Invalid(message)) = self;
        f.write_str(message)
    }
}

impl From<String> for InputFailure {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl From<&str> for InputFailure {
    fn from(message: &str) -> Self {
        Self::Invalid(message.into())
    }
}

/// Put each recorded input into `work` under its project-relative path.
fn materialise(
    root: &Path,
    source_root: &Path,
    work: &Path,
    inputs: &[(String, String)],
) -> std::result::Result<(), InputFailure> {
    let stage = tempfile::tempdir_in(work).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (rel, hash) in inputs {
        let mut local = scratch_key(rel)?;
        let Some(digest) = hash.strip_prefix("b3:") else {
            return Err("unsupported input hash".into());
        };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid input hash".into());
        }
        // Replay opens its documents as loose files: a recorded companion
        // goes where a loose document's companion is kept.
        if let Some((main, _)) = inputs.first()
            && rel != main
            && crate::files::is_companion_key(Path::new(rel), Path::new(main))
        {
            let record = crate::project::Project::document_record(
                None,
                &stage.path().join(scratch_key(main)?),
            );
            local = record
                .strip_prefix(stage.path())
                .map_err(|_| "companion outside the replay folder")?
                .to_path_buf();
        }
        let destination = stage.path().join(&local);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut candidates = vec![objects_dir(root).join(digest)];
        candidates.push(source_root.join(rel));
        let mut verified = false;
        for source in candidates
            .into_iter()
            .map(Some)
            .chain(std::iter::once_with(|| {
                crate::records::analysis_store::retained_source(root, hash)
                    .ok()
                    .flatten()
            }))
            .flatten()
        {
            // Verify while copying; a stale mutable link never masks another
            // valid candidate and never aliases the writable recovery output.
            if basecraft_io::write_atomic(&destination, |writer| {
                use std::io::Read;
                let mut input = std::fs::File::open(&source)?;
                let mut hasher = basecraft_io::Hasher::new();
                let mut buffer = [0_u8; 16 * 1024];
                loop {
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    hasher.update(&buffer[..count]);
                    writer.write_all(&buffer[..count])?;
                }
                if hasher.finish() != *hash {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "retained input hash differs",
                    ));
                }
                Ok(())
            })
            .is_ok()
            {
                verified = true;
                break;
            }
        }
        if !verified {
            return Err(InputFailure::Replaced(format!(
                "input {rel} changed or is missing (hash differs or unavailable); recovery requires the unchanged original or a verified retained copy"
            )));
        }
        written.push((destination, work.join(local)));
    }
    for (source, destination) in written {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(source, destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn scratch_key(key: &str) -> std::result::Result<PathBuf, String> {
    let path = Path::new(key);
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!("unsafe replay input path: {key}"));
    }
    if path.is_absolute() {
        let parent = path.parent().ok_or("input has no parent")?;
        let hash = basecraft_io::hash_bytes(parent.as_os_str().as_encoded_bytes());
        return Ok(PathBuf::from("external")
            .join(&hash[3..19])
            .join(path.file_name().ok_or("input has no filename")?));
    }
    if !safe_key(key) {
        return Err(format!("unsafe replay input path: {key}"));
    }
    Ok(path.to_path_buf())
}

fn safe_key(key: &str) -> bool {
    !key.is_empty()
        && Path::new(key)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Recovery sessions with a journal in the recovery folder of each of `roots`
/// (a project, or the folder of a loose file), oldest first. At most 256.
pub fn recovery_sessions(roots: &[std::path::PathBuf]) -> Result<Vec<std::path::PathBuf>> {
    let mut sessions = Vec::new();
    for root in roots {
        let entries = match std::fs::read_dir(crate::session::recovery_folder(root)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() && crate::log::log_dir(&entry.path()).is_dir() {
                sessions.push(entry.path());
            }
        }
    }
    if sessions.len() > 256 {
        return Err(limit("recovery sessions", 256));
    }
    // Session folders are named by ULID, which sorts by time.
    sessions.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    sessions.dedup();
    Ok(sessions)
}

/// Reconstruct unsaved documents into a new directory without modifying sources
/// or executing any tools. Strict reading rejects torn/corrupt journals; the
/// error names how many events [`recover_verified_prefix`] would keep.
/// This conservative wrapper drops its attempt on refusal. A returned check is
/// advisory only; continuations must use [`prepare_recovery`] on a live attempt.
pub fn recover(
    root: &Path,
    destination: &Path,
    project: Option<&crate::project::Project>,
) -> Result<Vec<std::path::PathBuf>> {
    let mut attempt = prepare_recovery(root, destination, RecoveryMode::Strict, project)?;
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

/// `project` is the project the user chose, if any: records of recovered
/// files inside it are indexed at its root.
pub fn prepare_recovery(
    root: &Path,
    destination: &Path,
    mode: RecoveryMode,
    project: Option<&crate::project::Project>,
) -> Result<RecoveryAttempt> {
    recover_checked(root, destination)?;
    let (entries, damage) = match mode {
        RecoveryMode::Strict => (strict_entries(root)?, None),
        RecoveryMode::VerifiedPrefix => {
            let prefix = log::verified_session_prefix(root)?;
            (prefix.entries, prefix.problem)
        }
    };
    recover_entries(root, destination, entries, damage, project)
}

fn strict_entries(root: &Path) -> Result<Vec<LogEntry>> {
    log::read_all(root).map_err(|error| {
        let kept = log::verified_session_prefix(root)
            .ok()
            .filter(|prefix| prefix.problem.is_some());
        match kept {
            Some(prefix) => damaged(root, format!(
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
    project: Option<&crate::project::Project>,
) -> Result<(Vec<std::path::PathBuf>, Option<String>)> {
    let mut attempt = prepare_recovery(root, destination, RecoveryMode::VerifiedPrefix, project)?;
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
    mut entries: Vec<LogEntry>,
    damage: Option<String>,
    project: Option<&crate::project::Project>,
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
    let mut session = Session::unlogged(staging.path())
        .in_project(project.cloned())
        .staged();
    let source_root = recorded_source_root(root, entries.iter());
    let pending_path = root.join("pending-save.json");
    if pending_path.is_file() {
        let pending: basecraft_protocol::records::PendingSave =
            basecraft_protocol::records::decode(&std::fs::read(&pending_path)?)?;
        if !pending.baseline.result.hashes.is_empty()
            && pending.baseline.result.hashes.iter().all(|(path, hash)| {
                scratch_key(path).is_ok()
                    && basecraft_io::hash_file(&source_root.join(path))
                        .ok()
                        .as_ref()
                        == Some(hash)
            })
        {
            let index = entries
                .iter()
                .position(|entry| {
                    entry.session == pending.baseline.session && entry.id > pending.baseline.id
                })
                .unwrap_or(entries.len());
            entries.insert(index, pending.baseline);
        }
    }

    let mut documents = std::collections::BTreeMap::<String, DocId>::new();
    let mut baselines = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        if matches!(
            entry.op,
            Op::Open { .. }
                | Op::CreateSequence { .. }
                | Op::CreateSangerReview { .. }
                | Op::Save { .. }
        ) {
            baselines.entry(entry.doc.clone()).or_default().push(index);
        }
    }
    // Open documents in journal order of their latest baseline.
    let mut baselines: Vec<_> = baselines.into_iter().collect();
    baselines.sort_by_key(|(_, candidates)| candidates.last().copied());
    let mut selected = HashMap::new();
    let mut skipped = Vec::new();
    for (key, candidates) in baselines {
        let mut failures = Vec::new();
        for index in candidates.into_iter().rev() {
            let entry = &entries[index];
            let damaged = |detail| damaged(root, detail);
            scratch_key(&key).map_err(damaged)?;
            let opened = match &entry.op {
                Op::CreateSequence { name, residues } => {
                    session.create_sequence_with_key(&key, name, residues)
                }
                Op::CreateSangerReview { review } => session
                    .create_sanger_review_with_key(&key, &review_record_at(root, review))
                    .inspect(|&doc| {
                        session.restore_replay_paths(doc, staging.path(), &source_root);
                    }),
                Op::Open { path } | Op::Save { path } => {
                    if !entry.result.hashes.iter().any(|(key, _)| key == path) {
                        return Err(damaged("unsafe or inconsistent recovery baseline".into()));
                    }
                    let local = staging.path().join(scratch_key(path).map_err(damaged)?);
                    match materialise(root, &source_root, staging.path(), &entry.result.hashes) {
                        Ok(()) => session.open(&local).and_then(|doc| {
                            let source = entry.source.as_ref().ok_or_else(|| {
                                damaged("Recovery baseline has no source record".into())
                            })?;
                            session.restore_recovery_ids(doc, source)?;
                            let original = source_root.join(path);
                            session.restore_replay_paths(
                                doc,
                                local.parent().expect("scratch parent"),
                                original.parent().expect("source parent"),
                            );
                            Ok(doc)
                        }),
                        Err(detail) => Err(damaged(detail.to_string())),
                    }
                }
                _ => unreachable!("baseline operation"),
            };
            match opened {
                Ok(doc)
                    if session
                        .state(doc)
                        .is_ok_and(|state| state.matches_recorded(&entry.state)) =>
                {
                    documents.insert(key.clone(), doc);
                    selected.insert(key.clone(), index);
                    break;
                }
                Ok(_) => failures.push("Recovery baseline state differs".into()),
                Err(error) => failures.push(error.to_string()),
            }
        }
        if !documents.contains_key(&key) {
            skipped.push(format!("{key}: {}", failures.join("; ")));
        }
    }
    if documents.is_empty() {
        return Err(damaged(root, skipped.join("\n")));
    }
    let damage = match (damage, skipped.is_empty()) {
        (damage, true) => damage,
        (previous, false) => Some(format!(
            "{}Recovery skipped documents; journals are unchanged:\n{}",
            previous.map_or(String::new(), |text| text + "\n"),
            skipped.join("\n")
        )),
    };
    for (index, entry) in entries.into_iter().enumerate() {
        let Some(baseline) = selected.get(&entry.doc) else {
            continue;
        };
        if index <= *baseline
            || matches!(
                entry.op,
                Op::Open { .. }
                    | Op::CreateSequence { .. }
                    | Op::CreateSangerReview { .. }
                    | Op::Save { .. }
            )
        {
            continue;
        }
        let doc = documents[&entry.doc];
        match entry.op {
            Op::Save { .. }
            | Op::ToolRun { .. }
            | Op::JobFinished { .. }
            | Op::ProposalOffer { .. }
            | Op::ProposalDecide { .. }
            | Op::Export { .. }
            | Op::Extract { .. } => {}
            Op::Undo { .. } => {
                session.undo_single(doc)?;
            }
            Op::Redo { .. } => {
                session.redo_single(doc)?;
            }
            operation => {
                session.apply_as(doc, sanger_record_at(root, operation), entry.actor)?;
            }
        }
        if !session.state(doc)?.matches_recorded(&entry.state) {
            return Err(damaged(
                root.join("events.jsonl"),
                format!("recovery state differs at {}", entry.id),
            ));
        }
    }
    session.recovery_placement(staging.path().to_path_buf(), destination.clone());
    let documents = documents
        .into_values()
        .map(|id| {
            Ok(RecoveryDocument {
                relative: session.doc_key(id)?.to_owned(),
                id,
                saved: None,
                written: false,
            })
        })
        .collect::<Result<_>>()?;
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
    })
}

/// Lifetime owner only. Session Slots retain all receipt authority. Dropping or
/// cancelling removes only this attempt's disposable stage; originals survive.
/// A failed pre-publication call leaves the attempt available for a fresh check.
pub struct RecoveryAttempt {
    session: Session,
    staging: Option<tempfile::TempDir>,
    destination: PathBuf,
    parent_identity: ObjectIdentity,
    documents: Vec<RecoveryDocument>,
    artifacts: Vec<ArtifactObservation>,
    warnings: Vec<crate::session::SaveWarning>,
    damage: Option<String>,
    published: bool,
}

struct RecoveryDocument {
    relative: String,
    id: DocId,
    // Expected outcome, never authority. This observation has no published token.
    saved: Option<crate::session::SaveCheck>,
    // A successful writer must never run twice even if its post-write
    // observation fails. That case requires a fresh finalization check.
    written: bool,
}

/// Stored workflow progress, not a fresh inspection or permission to proceed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    /// A writer has not completed. A failed write may still have partial effects.
    PendingWrite,
    /// The writer completed; only finalization remains. Evidence may need fresh
    /// consent, or an unobservable/changed stage may require reconstruction.
    NeedsFinalization,
    /// The directory rename completed. Sync/registration may subsequently have
    /// failed; use the publish result for completion or `PublishedWithFailure`.
    Published,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RecoveryDocumentStatus {
    /// Index accepted by this attempt's check/decide/advance methods, not a Slot id.
    pub index: usize,
    pub relative_key: String,
    pub phase: RecoveryPhase,
    /// True once the writer succeeded, even when its later observation failed.
    pub written: bool,
}

/// In-memory snapshot only. No receipt, Session access or validation authority.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecoveryStatus {
    pub destination: PathBuf,
    pub phase: RecoveryPhase,
    pub documents: Vec<RecoveryDocumentStatus>,
    pub damage: Option<String>,
    pub warnings: Vec<crate::session::SaveWarning>,
    pub evidence_actions: Vec<crate::session::EvidenceAction>,
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
    identity: ObjectIdentity,
    revision: Option<(String, basecraft_protocol::provenance::FileStamp)>,
}

pub(crate) fn directory_identity(path: &Path) -> Result<ObjectIdentity> {
    Ok(object_identity(path, OrdinaryKind::Directory)?)
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
                object_identity(path, OrdinaryKind::File)?,
                Some((
                    basecraft_io::hash_file(path)?,
                    crate::records::input_evidence::stamp(path)?,
                )),
            )
        } else {
            return Err(crate::error::invalid("unsafe recovery artifact"));
        };
        result.push(ArtifactObservation {
            path: basecraft_platform::files::inside(path, root)
                .ok_or_else(|| crate::error::invalid("artifact outside recovery stage"))?,
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
    /// Read stored progress without filesystem IO, hashing, validation or
    /// minting/consuming a receipt. `NeedsFinalization` is not a readiness claim:
    /// only publish can validate current artifacts/evidence and promote them.
    /// Published does not distinguish successful completion from post-rename
    /// failure; those details remain in the existing publish result.
    pub fn status(&self) -> Result<RecoveryStatus> {
        let phase = |written| {
            if self.published {
                RecoveryPhase::Published
            } else if written {
                RecoveryPhase::NeedsFinalization
            } else {
                RecoveryPhase::PendingWrite
            }
        };
        Ok(RecoveryStatus {
            destination: self.destination.clone(),
            phase: phase(self.documents.iter().all(|doc| doc.written)),
            documents: self
                .documents
                .iter()
                .enumerate()
                .map(|(index, doc)| RecoveryDocumentStatus {
                    index,
                    relative_key: doc.relative.clone(),
                    phase: phase(doc.written),
                    written: doc.written,
                })
                .collect(),
            damage: self.damage.clone(),
            warnings: self.warnings.clone(),
            evidence_actions: self.evidence_actions()?,
        })
    }

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
        self.session.recovery_check(doc.id, doc.written)
    }

    pub fn decide(
        &self,
        document: usize,
        token: &str,
        known: crate::records::input_evidence::SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<crate::session::SaveDecision> {
        self.validate()?;
        let doc = self.document(document)?;
        match self.session.decide_save(
            doc.id,
            Some(&self.session_path(document)?),
            token,
            known,
            acknowledge_incomplete,
        ) {
            Err(crate::Error::SaveDecisionStale(_)) => Err(crate::Error::SaveDecisionStale(
                Box::new(self.session.recovery_check(doc.id, doc.written)?),
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
        // A refusal before saving withdraws the consent; a failed withdrawal wins.
        let refuse = |session: &mut Session, error: Error| {
            decision
                .as_ref()
                .map(|d| session.discard_recovery_decision(d))
                .transpose()
                .err()
                .unwrap_or(error)
        };
        if let Err(error) = self
            .validate()
            .and_then(|_| self.document(document).map(|_| ()))
        {
            return Err(refuse(&mut self.session, error));
        }
        let doc = self.document(document)?;
        let id = doc.id;
        let _target = match crate::records::save_guard::lock(&self.destination) {
            Ok(lock) => lock,
            Err(error) => return Err(refuse(&mut self.session, error)),
        };
        let finalizing = doc.written;
        let observed = if doc.written {
            let _evidence =
                match crate::records::input_evidence::Mutation::begin_in(self.session.project()) {
                    Ok(guard) => guard,
                    Err(error) => return Err(refuse(&mut self.session, error)),
                };
            self.session
                .recovery_finalize(id, doc.saved.as_ref(), decision)?
        } else {
            let path = if decision.is_some() {
                Some(self.session_path(document)?)
            } else {
                None
            };
            let outcome = match decision {
                Some(decision) => self
                    .session
                    .save_with_decision(id, path.as_deref(), decision),
                None => self.session.save_recovery(id),
            };
            // Failed writes can leave partial owned output; Session retains its
            // existing acceptance rules. This is an observation, never consent.
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    if let Some(stage) = &self.staging
                        && let Ok(observed) = artifacts(stage.path())
                    {
                        self.artifacts = observed;
                    }
                    return Err(error);
                }
            };
            self.warnings.extend(outcome.warnings);
            self.documents[document].written = true;
            self.artifacts = artifacts(self.stage_path()?)?;
            outcome
                .recovery_check
                .ok_or_else(|| crate::error::invalid("recovery write observation missing"))??
        };
        if finalizing && !observed.complete() {
            self.warnings
                .push(crate::session::SaveWarning::EvidenceNotChecked {
                    issues: observed.issues(),
                });
        }
        self.documents[document].saved = Some(observed);
        self.artifacts = artifacts(self.stage_path()?)?;
        Ok(())
    }

    fn stage_path(&self) -> Result<&Path> {
        Ok(self
            .staging
            .as_ref()
            .ok_or_else(|| crate::error::invalid("recovery stage missing"))?
            .path())
    }

    fn session_path(&self, document: usize) -> Result<PathBuf> {
        Ok(self.stage_path()?.join(&self.document(document)?.relative))
    }

    pub fn publish(&mut self) -> Result<RecoveryCompletion> {
        let result = self.publish_inner();
        if !self.published
            && result.as_ref().is_err_and(|error| {
                !matches!(
                    error,
                    crate::Error::SaveCheckRequired(_) | crate::Error::SaveDecisionStale(_)
                )
            })
        {
            for doc in &mut self.documents {
                if let Some(observation) = &mut doc.saved {
                    observation.require_final_decision();
                }
            }
        }
        result
    }

    fn publish_inner(&mut self) -> Result<RecoveryCompletion> {
        self.validate()?;
        let _target = crate::records::save_guard::lock(&self.destination)?;
        let evidence = crate::records::input_evidence::Mutation::begin_in(self.session.project())?;
        for doc in &self.documents {
            if !doc.written {
                return Err(crate::error::invalid(
                    "recovery documents still need saving",
                ));
            }
            self.session
                .recovery_finalize(doc.id, doc.saved.as_ref(), None)?;
        }
        let stage = self.validate()?.to_path_buf();
        let stores = self.session.recovery_stores()?;
        let evidence_actions = self.session.recovery_actions()?;
        for artifact in self.artifacts.iter().rev() {
            let path = stage.join(&artifact.path);
            if artifact.revision.is_some() {
                basecraft_platform::files::sync_file(&path)?;
            } else {
                basecraft_platform::files::sync_directory(&path)?;
            }
        }
        self.validate()?;
        basecraft_platform::atomic::rename_noreplace(&stage, &self.destination)?;
        // Disarm immediately: post-promotion errors must never imply rollback.
        if let Some(staging) = self.staging.take() {
            let _ = staging.keep();
        }
        self.published = true;
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
            // The stage was a loose folder; each recovered document's
            // companion now moves to where the project keeps it.
            for output in &completion.outputs {
                if let Some(project) = self.session.project() {
                    crate::records::ArtifactRun::settle_binding(project, output)?;
                }
                crate::files::settle_companion(self.session.project(), output)?;
            }
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
mod tests {
    use super::*;

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
            materialise(root.path(), root.path(), work.path(), &inputs).unwrap();
            assert_eq!(std::fs::read(&dest).unwrap(), bytes);
            // Editing scratch must also be independent of the retained inode.
            std::fs::write(&dest, "scratch edit").unwrap();
            assert_eq!(basecraft_io::hash_file(&source).unwrap(), hash);
            materialise(root.path(), root.path(), work.path(), &inputs).unwrap();
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
        assert!(materialise(root.path(), root.path(), work.path(), &inputs).is_err());
        assert_eq!(std::fs::read(&object).unwrap(), b"corrupt");
        assert_eq!(std::fs::read(&dest).unwrap(), b"previous");
        std::fs::remove_file(&object).unwrap();
        assert!(materialise(root.path(), root.path(), work.path(), &inputs).is_err());
        std::fs::write(root.path().join("a.fa"), "changed").unwrap();
        assert!(materialise(root.path(), root.path(), work.path(), &inputs).is_err());
        // Force atomic persistence failure after the temporary copy is written.
        std::fs::write(root.path().join("a.fa"), "expected").unwrap();
        std::fs::remove_file(&dest).unwrap();
        std::fs::create_dir(&dest).unwrap();
        assert!(materialise(root.path(), root.path(), work.path(), &inputs).is_err());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(root.path().join("a.fa")).unwrap(),
            b"expected"
        );
        for rel in ["../escape", "/absolute"] {
            assert!(
                materialise(
                    root.path(),
                    root.path(),
                    work.path(),
                    &[(rel.into(), inputs[0].1.clone())]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn refused_later_baseline_falls_back_to_the_earlier_one() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.fa");
        std::fs::write(&path, ">a\nACGT\n").unwrap();
        let mut session = Session::new(root.path());
        let doc = session.open(&path).unwrap();
        session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
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
            let mut report = ReplayReport::default();
            replay_session(
                root.path(),
                &[baseline, &attempt, edit],
                &HashMap::new(),
                &mut report,
            )
            .unwrap();
            if attempt.id == baseline.id && attempt.op == baseline.op {
                // A missing original is not a problem: the baseline and the
                // edit after it are not checkable.
                assert!(report.ok(), "{report:#?}");
                assert_eq!(report.unchecked, 2, "{report:#?}");
            } else {
                assert_eq!(report.problems.len(), 2, "{report:#?}");
                assert!(
                    report.problems[1].contains("document wasn't opened"),
                    "{report:#?}"
                );
            }
            let destination = root.path().join("recovered");
            let escapes = matches!(&attempt.op, Op::Open { path } if path.starts_with(".."));
            let recovered = recover_entries(
                root.path(),
                &destination,
                vec![baseline.clone(), attempt, edit.clone()],
                None,
                None,
            );
            if escapes {
                // A journal naming a path outside its root is damaged.
                assert!(recovered.is_err());
            } else {
                // A refused later baseline falls back to the earlier one; the
                // edit's recorded state still has to match.
                assert_eq!(recovered.unwrap().documents.len(), 1);
            }
            assert!(!destination.exists());
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".basecraft-recover-")
            }));
        }
    }

    fn logged(id: &str, doc: &str, op: Op) -> LogEntry {
        LogEntry {
            source: None,
            v: basecraft_protocol::ops::LOG_VERSION,
            id: id.into(),
            t: "2026-11-13T12:00:00Z".into(),
            host: "host".into(),
            session: "session".into(),
            actor: basecraft_protocol::Actor::External,
            doc: doc.into(),
            op,
            result: basecraft_protocol::OpResult::default(),
            state: basecraft_protocol::DocState {
                len: 0,
                features: 0,
                rows: None,
                counts: None,
            },
        }
    }

    fn offer(id: &str, doc: &str, offered: usize) -> LogEntry {
        logged(
            id,
            doc,
            Op::ProposalOffer {
                proposal: "p".into(),
                source: basecraft_protocol::ops::ProposalSource::Run { id: "run".into() },
                base_revision: 0,
                operations: vec![Op::seq_replace(0, 1, "A"); offered],
            },
        )
    }

    fn decide(id: &str, doc: &str, outcome: ProposalDecision, applied: &[&str]) -> LogEntry {
        logged(
            id,
            doc,
            Op::ProposalDecide {
                proposal: "p".into(),
                outcome,
                automatic: true,
                applied: applied.iter().map(|id| (*id).into()).collect(),
                note: None,
            },
        )
    }

    /// The problems `Proposals` finds in `entries`, undecided offers last.
    fn proposal_problems(entries: &[LogEntry]) -> Vec<String> {
        let mut proposals = Proposals::default();
        let mut problems: Vec<_> = entries.iter().filter_map(|e| proposals.check(e)).collect();
        problems.extend(proposals.undecided().map(|p| format!("{p} undecided")));
        problems
    }

    #[test]
    fn proposal_entries_are_checked_for_consistency() {
        use ProposalDecision::{Accepted, Modified, Rejected};
        let edit = |id| logged(id, "a.fa", Op::seq_replace(0, 1, "T"));
        let consistent = [
            offer("o", "a.fa", 2),
            edit("e1"),
            edit("e2"),
            decide("d", "a.fa", Accepted, &["e1", "e2"]),
        ];
        assert_eq!(proposal_problems(&consistent), Vec::<String>::new());
        let rejected = [offer("o", "a.fa", 1), decide("d", "a.fa", Rejected, &[])];
        assert_eq!(proposal_problems(&rejected), Vec::<String>::new());
        let not_exactly = "applied isn't exactly the edits between";
        let not_offered = "the edits applied aren't the edits offered";
        let human = |entry: LogEntry| LogEntry {
            actor: basecraft_protocol::Actor::Human { user: "u".into() },
            ..entry
        };
        let cases: [(&[LogEntry], &str); 11] = [
            (
                &[decide("d", "a.fa", Rejected, &[])],
                "wasn't offered before",
            ),
            (&[offer("o", "a.fa", 1)], "undecided"),
            (
                &[
                    edit("e1"),
                    offer("o", "a.fa", 1),
                    decide("d", "a.fa", Accepted, &["e1"]),
                ],
                not_exactly,
            ),
            (
                &[
                    offer("o", "a.fa", 1),
                    decide("d", "a.fa", Accepted, &["missing"]),
                ],
                not_exactly,
            ),
            (
                &[
                    offer("o", "a.fa", 2),
                    edit("e1"),
                    decide("d", "a.fa", Accepted, &["e1"]),
                ],
                not_offered,
            ),
            (
                &[
                    offer("o", "a.fa", 1),
                    edit("e1"),
                    decide("d", "a.fa", Rejected, &["e1"]),
                ],
                "a rejection lists applied edits",
            ),
            (
                &[offer("o", "a.fa", 1), decide("d", "a.fa", Modified, &[])],
                "nothing applied",
            ),
            (
                &[offer("o", "a.fa", 1), decide("d", "b.fa", Rejected, &[])],
                "was offered for a.fa",
            ),
            // An edit logged between offer and decision that it doesn't list.
            (
                &[
                    offer("o", "a.fa", 2),
                    edit("e1"),
                    edit("e2"),
                    decide("d", "a.fa", Modified, &["e1"]),
                ],
                not_exactly,
            ),
            // The user's own edit claimed by a plugin's decision.
            (
                &[
                    offer("o", "a.fa", 1),
                    human(edit("h1")),
                    decide("d", "a.fa", Accepted, &["h1"]),
                ],
                "isn't the offerer's edit",
            ),
            // Accepted, but something other than the offer was applied.
            (
                &[
                    offer("o", "a.fa", 1),
                    logged("r1", "a.fa", Op::feature_remove(0)),
                    decide("d", "a.fa", Accepted, &["r1"]),
                ],
                not_offered,
            ),
        ];
        for (entries, expected) in cases {
            let problems = proposal_problems(entries);
            assert!(
                problems.iter().any(|problem| problem.contains(expected)),
                "{expected}: {problems:?}"
            );
        }
        let twice = [
            offer("o", "a.fa", 1),
            decide("d", "a.fa", Rejected, &[]),
            decide("d2", "a.fa", Rejected, &[]),
        ];
        assert!(proposal_problems(&twice)[0].contains("decided twice"));
    }
}

#[cfg(test)]
mod attempt_boundary_tests;
fn review_record_at(
    root: &Path,
    record: &basecraft_protocol::sanger_records::ReviewRecord,
) -> basecraft_protocol::sanger_records::ReviewRecord {
    basecraft_protocol::sanger_records::ReviewRecord {
        path: objects_dir(root).join(record.hash.trim_start_matches("b3:")),
        hash: record.hash.clone(),
    }
}
fn sanger_record_at(root: &Path, mut op: Op) -> Op {
    if let Op::SangerUpdate { review } = &mut op {
        *review = review_record_at(root, review);
    }
    op
}
