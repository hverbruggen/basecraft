//! Inspectable portable save history. No bundle content is ever executed.
use crate::error::{damaged, limit, unsupported};
use crate::{Result, error::invalid};
use basecraft_protocol::{
    Actor, DocState, LogEntry, OpResult,
    provenance::{ArtifactRevision, Completeness, Publication},
};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// The saved history of a loose file: in the `.basecraft` beside it,
/// never beside the file itself (D23).
pub fn companion(path: &Path) -> PathBuf {
    crate::project::Project::provenance_record(None, path)
}

/// Select the existing binding or companion path.
/// The final fallback is the original companion spelling, even if it does not
/// exist. Selection does not validate content or establish complete provenance.
pub fn bundle_for_result(result: &Path, project: Option<&crate::project::Project>) -> PathBuf {
    if let Ok(Some(hidden)) = crate::records::analysis_store::ArtifactRun::for_result(result)
        && hidden.join("manifest.json").is_file()
    {
        return hidden;
    }
    let standalone = companion(result);
    if standalone.join("manifest.json").is_file() {
        return standalone;
    }
    let absolute = result
        .canonicalize()
        .unwrap_or_else(|_| result.to_path_buf());
    // The project's bundle counts only when it holds history.
    if let Some(bundle) = project.and_then(|project| project.provenance_bundle(&absolute))
        && bundle.join("manifest.json").is_file()
    {
        return bundle;
    }
    standalone
}

fn safe_relative(root: &Path, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(invalid("provenance contains an unsafe relative path"));
    }
    let joined = root.join(p);
    // Reject symlink escape as well as lexical traversal.
    let canonical_root = root.canonicalize()?;
    let canonical = joined.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        return Err(invalid("provenance artifact escapes its bundle"));
    }
    Ok(canonical)
}

pub fn read_record(path: &Path) -> Result<Vec<u8>> {
    read_record_with_budget(path, &mut (64 << 20))
}

/// Share the per-record cap with callers enforcing an aggregate read budget.
pub(crate) fn read_record_with_budget(path: &Path, remaining: &mut u64) -> Result<Vec<u8>> {
    let file = basecraft_platform::files::open_regular_read(path)?;
    let length = file.metadata()?.len();
    read_record_from_open(file, length, remaining, 64 << 20)
}

pub(crate) fn read_record_from_open(
    file: impl std::io::Read,
    length: u64,
    remaining: &mut u64,
    cap: u64,
) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let limit = (*remaining).min(64 << 20).min(cap);
    let too_long = || crate::error::limit("provenance record reading", limit);
    if length > limit {
        return Err(too_long());
    }
    let mut bytes = Vec::new();
    let result = file.take(limit + 1).read_to_end(&mut bytes);
    *remaining = remaining.saturating_sub(bytes.len() as u64);
    result?;
    if bytes.len() as u64 > limit {
        return Err(too_long());
    }
    Ok(bytes)
}

/// A file a run keeps, as its publication lists it: `<run id>:<label>`, the
/// name shown, its bytes and where in the run folder they are.
pub fn retained_file(
    run: &crate::records::ArtifactRun,
    label: &str,
    logical_name: String,
    path: &Path,
    hash: String,
) -> Result<ArtifactRevision> {
    Ok(ArtifactRevision {
        id: format!("{}:{label}", run.id()),
        logical_name,
        hash,
        size: std::fs::metadata(path)?.len(),
        retained: Some(crate::records::relative(run.directory(), path)?),
        reference: None,
    })
}

/// Move a run's file to a new file in the project, never replacing one
/// (D26). The receipt names the file's inode before the move, so a later
/// repair adopts only the file this run moved.
pub fn move_file(source: &Path, destination: &Path, hash: &str, receipt: &Path) -> Result<()> {
    let check = |file: &Path| {
        if basecraft_io::hash_file(file)? == hash {
            Ok(())
        } else {
            Err(invalid("member changed before export"))
        }
    };
    check(source)?;
    move_into_project(
        source,
        destination,
        |file| write_receipt(receipt, destination, hash, file),
        check,
    )
}

/// Move a run's file or folder to `destination`, which must not exist (D26).
/// `mark` records what is about to become `destination` (a receipt or an
/// intent) before it appears there. Across file systems the source is
/// copied to a temporary in the destination's folder, synced, checked with
/// `check`, marked and renamed into place, and only then removed, so one
/// copy remains.
pub fn move_into_project(
    source: &Path,
    destination: &Path,
    mark: impl Fn(&Path) -> Result<()>,
    check: impl Fn(&Path) -> Result<()>,
) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("missing destination parent"))?;
    mark(source)?;
    match basecraft_platform::atomic::rename_noreplace(source, destination) {
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            let folder = source.is_dir();
            let pending = tempfile::Builder::new()
                .prefix(".basecraft-export-")
                .tempdir_in(parent)?;
            let copy = pending.path().join("moving");
            if folder {
                copy_tree(source, &copy)?;
            } else {
                copy_atomic(source, &copy)?;
            }
            basecraft_platform::files::sync_directory(pending.path())?;
            check(&copy)?;
            mark(&copy)?;
            no_replace(basecraft_platform::atomic::rename_noreplace(
                &copy,
                destination,
            ))?;
            if folder {
                std::fs::remove_dir_all(source)?;
            } else {
                std::fs::remove_file(source)?;
            }
        }
        moved => no_replace(moved)?,
    }
    basecraft_platform::files::sync_directory(parent)?;
    if let Some(folder) = source.parent() {
        basecraft_platform::files::sync_directory(folder)?;
    }
    Ok(())
}

fn no_replace(renamed: std::io::Result<()>) -> Result<()> {
    renamed.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            crate::access::AccessError::DestinationExists.into()
        } else {
            crate::Error::from(error)
        }
    })
}

/// The receipt of a move: `file` is the file that becomes `destination`.
fn write_receipt(receipt: &Path, destination: &Path, hash: &str, file: &Path) -> Result<()> {
    let (device, inode) = basecraft_platform::files::identity(file)?;
    crate::records::write_json(
        receipt,
        &serde_json::to_value(
            basecraft_protocol::publication_records::MemberExportReceipt {
                schema: 1,
                destination: destination.to_path_buf(),
                hash: hash.into(),
                device,
                inode,
                size: std::fs::metadata(file)?.len(),
            },
        )?,
    )
}

/// Whether `destination` is still the file [`move_file`] put there, with the
/// same bytes. Matching bytes alone never authorize adopting another file.
pub fn verify_export(destination: &Path, hash: &str, receipt: &Path) -> Result<()> {
    use basecraft_platform::files::{identity, link_count};
    let receipt: basecraft_protocol::publication_records::MemberExportReceipt =
        basecraft_protocol::records::decode(&read_record(receipt)?)?;
    let metadata = std::fs::symlink_metadata(destination)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || link_count(destination)? != 1
        || receipt.schema != 1
        || receipt.destination != destination
        || receipt.hash != hash
        || (receipt.device, receipt.inode) != identity(destination)?
        || receipt.size != metadata.len()
        || basecraft_io::hash_file(destination)? != hash
    {
        return Err(damaged(
            destination,
            "member ownership or bytes changed; repair refused",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "provenance/record_access_tests.rs"]
mod record_access_tests;

fn copy_atomic(source: &Path, target: &Path) -> Result<()> {
    basecraft_io::write_atomic(target, |writer| {
        let mut input = std::fs::File::open(source)?;
        std::io::copy(&mut input, writer)?;
        Ok(())
    })?;
    Ok(())
}

pub fn inspect(result: &Path, project: Option<&crate::project::Project>) -> Result<Publication> {
    // A damaged hidden binding is an error, never a reason to use stale legacy context.
    crate::records::analysis_store::ArtifactRun::for_result(result)?;
    let bundle = bundle_for_result(result, project);
    let manifest = bundle.join("manifest.json");
    if project.is_none() && !manifest.is_file() {
        // Never guess a project; say where else the history could be.
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No history beside this file. If it belongs to a project, choose that project \
             (CLI and MCP: --project).",
        )
        .into());
    }
    latest(&bundle)?
        .map(|(publication, _)| publication)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No saved history for this file",
            )
            .into()
        })
}

/// The latest record of a history folder and the hash of its bytes, through
/// `manifest.json`; `None` when the folder has no history.
pub fn latest(bundle: &Path) -> Result<Option<(Publication, String)>> {
    let manifest = bundle.join("manifest.json");
    if !manifest.is_file() {
        return Ok(None);
    }
    let pointer: basecraft_protocol::provenance::LatestRecord =
        basecraft_protocol::records::decode(&read_record(&manifest)?)?;
    if pointer.schema_version != 1 {
        return Err(unsupported("unsupported provenance schema version"));
    }
    let (publication, hash) = read_publication(bundle, &pointer.id, &mut (64 << 20))?;
    if hash != pointer.hash {
        return Err(damaged(
            record_path(bundle, &pointer.id)?,
            "the latest history record changed after it was saved",
        ));
    }
    Ok(Some((publication, hash)))
}

/// [`latest`] for a folder that must have history.
pub fn latest_record(bundle: &Path) -> Result<(Publication, String)> {
    latest(bundle)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} holds no saved history", bundle.display()),
        )
        .into()
    })
}

/// `records/<id>.json` in a history folder; refuses an id that is not a
/// plain record name.
pub(crate) fn record_path(bundle: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(invalid("unsafe publication identity"));
    }
    Ok(bundle.join("records").join(format!("{id}.json")))
}

/// One record and the hash of its bytes, counted against `budget`.
fn read_publication(bundle: &Path, id: &str, budget: &mut u64) -> Result<(Publication, String)> {
    let bytes = read_record_with_budget(&record_path(bundle, id)?, budget)?;
    let publication: Publication = basecraft_protocol::records::decode(&bytes)?;
    if publication.id != id || publication.schema_version != 1 {
        return Err(invalid(
            "unsupported or inconsistent history record identity or version",
        ));
    }
    Ok((publication, basecraft_io::hash_bytes(&bytes)))
}

/// A parent record that could not be read: history before it is unknown.
#[derive(Clone, Debug)]
pub struct HistoryGap {
    pub record: String,
    /// When the record that names it was saved.
    pub before: String,
    pub reason: &'static str,
}

impl HistoryGap {
    pub fn describe(&self) -> String {
        format!(
            "History before {} is incomplete: the saved record {} {}.",
            self.before, self.record, self.reason
        )
    }
}

/// `latest` and the records it descends from, each after its parents. A
/// parent that is missing or whose bytes differ from the hash its child
/// recorded is a gap, not an error.
fn ancestry(bundle: &Path, latest: Publication) -> Result<(Vec<Publication>, Vec<HistoryGap>)> {
    /// A record still to read (its id, the hash its child expects and when
    /// the child was saved), one to expand, or one whose parents are done.
    enum Step {
        Read(String, Option<String>, String),
        Expand(Publication),
        Done(Publication),
    }
    let mut budget = 128u64 << 20;
    // A record is visited when it is expanded (or found unreadable), so one
    // reached through two children still comes after all its own parents.
    let mut visited = std::collections::HashSet::new();
    let mut gaps = Vec::new();
    let mut order = Vec::new();
    let mut stack = vec![Step::Expand(latest)];
    while let Some(step) = stack.pop() {
        let record = match step {
            Step::Done(record) => {
                order.push(record);
                continue;
            }
            Step::Expand(record) => record,
            Step::Read(id, expected, before) => {
                if visited.contains(&id) {
                    continue;
                }
                let read = read_publication(bundle, &id, &mut budget);
                let reason = match read {
                    Ok((parent, hash)) if expected.as_deref() == Some(hash.as_str()) => {
                        stack.push(Step::Expand(parent));
                        continue;
                    }
                    Ok(_) => "has changed since it was saved",
                    Err(crate::Error::Io(error))
                        if error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        "is missing"
                    }
                    Err(error @ crate::Error::Limit { .. }) => return Err(error),
                    Err(_) => "cannot be read",
                };
                visited.insert(id.clone());
                gaps.push(HistoryGap {
                    record: id,
                    before,
                    reason,
                });
                continue;
            }
        };
        if !visited.insert(record.id.clone()) {
            continue;
        }
        if visited.len() > 10_000 {
            return Err(limit("history records", 10_000));
        }
        let mut parents = std::collections::HashSet::new();
        let reads: Vec<_> = record
            .parents
            .iter()
            .map(|id| {
                if !parents.insert(id) {
                    return Err(invalid("duplicate parent identity"));
                }
                let expected = record.parent_hashes.get(id).cloned();
                Ok(Step::Read(id.clone(), expected, record.created.clone()))
            })
            .collect::<Result<_>>()?;
        stack.push(Step::Done(record));
        // The first parent is read first.
        stack.extend(reads.into_iter().rev());
    }
    Ok((order, gaps))
}

/// A document's saved history: every record up to the latest, read through
/// the parent links, oldest first.
#[derive(Clone, Debug, Default)]
pub struct SavedHistory {
    pub events: Vec<LogEntry>,
    /// Edits in effect in the latest revision, oldest first.
    pub effective: Vec<basecraft_protocol::provenance::EditActivity>,
    pub gaps: Vec<HistoryGap>,
}

impl SavedHistory {
    /// Read the records `latest` descends from. A parent that is missing or
    /// whose bytes differ from its recorded hash is a gap, not an error.
    /// The documents a folder's records were made from come first: the
    /// records its input record links to (an analysis run's input, a Save As
    /// source), read the same way.
    pub fn read(bundle: &Path, latest: Publication) -> Result<Self> {
        let mut history = Self::default();
        history.read_into(bundle, latest, &mut HashSet::new(), &mut HashSet::new())?;
        Ok(history)
    }

    fn read_into(
        &mut self,
        bundle: &Path,
        latest: Publication,
        events: &mut HashSet<String>,
        records: &mut HashSet<(PathBuf, String)>,
    ) -> Result<()> {
        let upstream = crate::records::input_evidence::read_input(bundle)
            .map(|input| input.upstream)
            .unwrap_or_default();
        for link in upstream {
            let folder = bundle.join(&link.bundle);
            let folder = folder.canonicalize().unwrap_or(folder);
            if !records.insert((folder.clone(), link.id.clone())) {
                continue;
            }
            if records.len() > 10_000 {
                return Err(limit("history records", 10_000));
            }
            let gap = |reason| HistoryGap {
                record: link.id.clone(),
                before: latest.created.clone(),
                reason,
            };
            match read_publication(&folder, &link.id, &mut (64 << 20)) {
                Ok((input, hash)) if hash == link.hash => {
                    self.read_into(&folder, input, events, records)?;
                }
                Ok(_) => self.gaps.push(gap("has changed since it was saved")),
                Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.gaps.push(gap("is missing"));
                }
                Err(error @ crate::Error::Limit { .. }) => return Err(error),
                Err(_) => self.gaps.push(gap("cannot be read")),
            }
        }
        let (order, gaps) = ancestry(bundle, latest)?;
        self.gaps.extend(gaps);
        for mut record in order {
            record
                .events
                .retain(|event| events.insert(event.id.clone()));
            self.add(record);
        }
        Ok(())
    }

    /// The history of a record with no parents.
    pub fn of(record: Publication) -> Self {
        let mut history = Self::default();
        history.add(record);
        history
    }

    /// Add a record that descends from everything already in the history.
    pub(crate) fn add(&mut self, record: Publication) {
        let withdrawn: std::collections::HashSet<_> = record.withdrawn.into_iter().collect();
        self.effective.retain(|edit| !withdrawn.contains(&edit.id));
        for edit in record.effective_edits {
            if !self.effective.iter().any(|known| known.id == edit.id) {
                self.effective.push(edit);
            }
        }
        self.events.extend(record.events);
    }
}

pub fn verify(result: &Path, project: Option<&crate::project::Project>) -> Result<Vec<String>> {
    let publication = inspect(result, project)?;
    let bundle = bundle_for_result(result, project);
    let mut issues = verify_records(&bundle, &publication)?;
    if bundle.join("input.json").is_file() {
        if crate::records::input_evidence::read_input(&bundle)?.state
            == basecraft_protocol::input_records::EvidenceState::Broken
        {
            issues.push("Input evidence explicitly broken".into());
        }
        match crate::records::input_evidence::verify_ancestors(&bundle) {
            Ok(upstream) => issues.extend(upstream),
            Err(e) => issues.push(format!("Upstream evidence unavailable: {e}")),
        }
    }
    for artifact in &publication.inputs {
        if let Some(relative) = &artifact.retained {
            match safe_relative(&bundle, relative).and_then(|path| {
                let size = std::fs::metadata(&path)?.len();
                let hash = basecraft_io::hash_file(&path)?;
                Ok(size == artifact.size && hash == artifact.hash)
            }) {
                Ok(true) => {}
                Ok(false) => issues.push(format!("{}: retained bytes differ", artifact.id)),
                Err(e) => issues.push(format!("{}: {e}", artifact.id)),
            }
        } else if let Some(reference) = &artifact.reference {
            if artifact.hash != reference.hash || artifact.size != reference.stamp.size {
                issues.push(format!(
                    "{}: input revision/reference identity differs",
                    artifact.id
                ));
            }
            if let Err(e) = crate::records::input_evidence::verified_source(&bundle, reference) {
                issues.push(format!("{}: {e}", artifact.id));
            }
        }
    }
    if publication.outputs.is_empty() {
        return Err(invalid("publication has no output"));
    }
    let hidden = crate::records::analysis_store::ArtifactRun::for_result(result)?.is_some();
    // The companion is published by its file name; it lives where
    // `companion_path` keeps it, not beside the result.
    let companion = crate::files::companion_path(project, result);
    let is_companion = |name: &str| {
        !hidden
            && companion
                .file_name()
                .is_some_and(|file| file.to_string_lossy() == name)
    };
    for (index, output) in publication.outputs.iter().enumerate() {
        let path = if index == 0 && !hidden {
            result.to_path_buf()
        } else if index > 0 && is_companion(&output.logical_name) {
            companion.clone()
        } else {
            let base = if output.retained.as_deref() == Some(output.logical_name.as_str()) {
                bundle.as_path()
            } else if hidden {
                bundle
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .ok_or_else(|| invalid("invalid hidden publication path"))?
            } else {
                result.parent().unwrap_or(Path::new("."))
            };
            safe_relative(base, &output.logical_name)?
        };
        match basecraft_io::hash_file(&path) {
            Ok(hash) if hash == output.hash => {}
            Ok(_) => issues.push(format!(
                "{} differs from its published revision",
                output.logical_name
            )),
            Err(error) => issues.push(format!("{} is unavailable: {error}", output.logical_name)),
        }
    }
    Ok(issues)
}

pub(crate) fn verify_records(bundle: &Path, current: &Publication) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    let (stored, _) = read_publication(bundle, &current.id, &mut (64 << 20))?;
    if serde_json::to_value(&stored)? != serde_json::to_value(current)? {
        issues.push(format!(
            "{}: immutable publication record differs",
            current.id
        ));
    }
    let (records, gaps) = ancestry(bundle, stored)?;
    issues.extend(
        gaps.iter()
            .map(|gap| format!("{}: ancestor record {}", gap.record, gap.reason)),
    );
    // Effective edits may name an event of an earlier record, such as an
    // edit redone after a save.
    let mut events = std::collections::HashMap::new();
    let mut artifact_bytes = 0u64;
    for publication in &records {
        let own: std::collections::HashSet<_> = publication.events.iter().map(|e| &e.id).collect();
        if own.len() != publication.events.len() {
            issues.push(format!("{}: duplicate event identities", publication.id));
        }
        events.extend(publication.events.iter().map(|e| (&e.id, &e.op)));
        for artifact in publication.inputs.iter().chain(&publication.outputs) {
            if let Some(relative) = &artifact.retained {
                let retained = safe_relative(bundle, relative)?;
                let size = std::fs::metadata(&retained)?.len();
                artifact_bytes = artifact_bytes.saturating_add(size);
                if artifact_bytes > 8u64 << 30 {
                    return Err(limit("ancestry artifact verification", 8 << 30));
                }
                if size != artifact.size || basecraft_io::hash_file(&retained)? != artifact.hash {
                    issues.push(format!("{}: ancestor retained bytes differ", artifact.id));
                }
            } else if let Some(reference) = &artifact.reference {
                if artifact.hash != reference.hash || artifact.size != reference.stamp.size {
                    issues.push(format!(
                        "{}: ancestor input revision/reference identity differs",
                        artifact.id
                    ));
                }
                if let Err(e) = crate::records::input_evidence::verified_source(bundle, reference) {
                    issues.push(format!("{}: {e}", artifact.id));
                }
            }
        }
    }
    for activity in records.iter().flat_map(|record| &record.effective_edits) {
        if events.get(&activity.id) != Some(&&activity.operation) {
            issues.push(format!(
                "{}: effective activity has no matching event",
                activity.id
            ));
        }
    }
    Ok(issues)
}

fn stage_json(
    prepared: &mut crate::documents::PreparedWrite,
    path: &Path,
    value: &impl serde::Serialize,
) -> Result<()> {
    let bytes = basecraft_protocol::records::encode(value)?;
    if bytes.len() > 64 << 20 {
        return Err(limit("provenance record publication", 64 << 20));
    }
    prepared.push(path, |w| w.write_all(&bytes))?;
    Ok(())
}

// Carry explicitly selected input runs into ordinary saved ancestry: a Sanger
// review chosen for an alignment (`sanger_input`) or any other `ToolRun` that
// names a registered reference ledger (`input_run`), such as imported support
// values. The event remains a ToolRun; edits, undo and replay are unchanged.
// Processed events stay listed under the original `sanger_input_events` key.
// `events` and `effective` are the whole history, the publication's own
// record included.
fn link_selected_inputs<'a>(
    bundle: &Path,
    publication: &Publication,
    events: impl Iterator<Item = &'a LogEntry> + Clone,
    effective: &[basecraft_protocol::provenance::EditActivity],
) -> Result<()> {
    let mut effective = effective.to_vec();
    crate::records::contributions::project(events.clone(), &mut effective);
    let retained: std::collections::HashSet<_> = effective.iter().map(|e| e.id.as_str()).collect();
    // Each selected input names the run that retains its exact bytes.
    let mut dependencies: Vec<(&String, basecraft_protocol::records::ActivityInputRun)> =
        Vec::new();
    for event in events {
        let basecraft_protocol::Op::ToolRun {
            input_run,
            contributions,
            ..
        } = &event.op
        else {
            continue;
        };
        if let Some(run) = input_run
            && (contributions.is_none() || retained.contains(event.id.as_str()))
        {
            dependencies.push((&event.id, run.clone()));
        }
    }
    let path = bundle.join("input.json");
    if dependencies.is_empty() && !path.is_file() {
        return Ok(());
    }
    let mut input = if path.is_file() {
        crate::records::input_evidence::read_input(bundle)?
    } else {
        let output = publication
            .outputs
            .first()
            .ok_or_else(|| invalid("input publication has no result"))?;
        crate::records::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: output.hash.clone(),
            submitted_size: output.size,
            transformation: "identity-v1".into(),
            state: "generated".into(),
            analysis_run: None,
            proposal_target: None,
            upstream: Vec::new(),
            origin_run: None,
            extra: std::collections::BTreeMap::new(),
        }
    };
    let mut processed = input
        .extra
        .get("sanger_input_events")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if publication
        .origin
        .starts_with("Portable export of revision ")
    {
        // The package already materialized and verified the entire ancestor
        // graph. Remember its historical selections without reconnecting an
        // export (or its later Save As) to the original live ledgers.
        for (event, _) in dependencies {
            if !processed.iter().any(|id| id.as_str() == Some(event)) {
                processed.push(serde_json::json!(event));
            }
        }
        input
            .extra
            .insert("sanger_input_events".into(), serde_json::json!(processed));
        return crate::records::input_evidence::write_json(&path, &input);
    }
    let mut upstream = input.upstream.clone();
    // Keep ownership of selected edges separate from unrelated scientific inputs.
    // Historical publications and their records are left intact.
    let mut links: std::collections::BTreeMap<
        String,
        basecraft_protocol::input_records::UpstreamLink,
    > = input
        .extra
        .get("selected_input_links")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    for reference in links.values_mut() {
        if let Some(current) = upstream
            .iter()
            .find(|current| current.id == reference.id && current.hash == reference.hash)
        {
            *reference = current.clone();
        }
    }
    let selected: std::collections::HashSet<_> = dependencies
        .iter()
        .map(|(event, _)| event.as_str())
        .collect();
    let expired: Vec<_> = links
        .iter()
        .filter(|(event, _)| !selected.contains(event.as_str()))
        .map(|(event, reference)| (event.clone(), reference.clone()))
        .collect();
    for (event, reference) in expired {
        links.remove(&event);
        if !links.values().any(|other| other == &reference) {
            upstream.retain(|other| other != &reference);
        }
        processed.retain(|id| id.as_str() != Some(event.as_str()));
    }
    let inherited = crate::records::input_evidence::ancestors(bundle)?;
    for (event, dependency) in dependencies {
        if processed.iter().any(|id| id.as_str() == Some(event)) {
            continue;
        }
        if dependency.schema_version != 1 {
            return Err(unsupported("Unsupported selected input dependency"));
        }
        let id = dependency.id.as_str();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid("Unsafe selected input identity"));
        }
        let expected = dependency.hash.as_str();
        // An existing relative ancestor remains valid after moving the folder;
        // never replace it with an obsolete absolute path from historical events.
        if let Some((directory, _)) = inherited.iter().find(|(_, p)| p.id == id) {
            if basecraft_io::hash_file(&directory.join("records").join(format!("{id}.json")))?
                != expected
            {
                return Err(invalid("Inherited selected input identity differs"));
            }
        } else {
            let directory = crate::records::input_evidence::registered_run(id, expected, bundle)?;
            let reference = basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::records::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &directory,
                )?)
                .into(),
                id: (id).to_owned(),
                hash: (expected).to_owned(),
            };
            if !upstream.contains(&reference) {
                upstream.push(reference.clone());
                links.insert(event.clone(), reference);
            } else if links.values().any(|owned| owned == &reference) {
                links.insert(event.clone(), reference);
            }
        }
        if let Some(reference) = upstream.iter().find(|r| r.id == id)
            && links.values().any(|owned| owned == reference)
        {
            links.insert(event.clone(), reference.clone());
        }
        processed.push(serde_json::json!(event));
    }
    input
        .extra
        .insert("selected_input_links".into(), serde_json::json!(links));
    input.upstream = upstream;
    input
        .extra
        .insert("sanger_input_events".into(), serde_json::json!(processed));
    crate::records::input_evidence::write_json(&path, &input)
}

/// Publish a finished analysis run into its own bundle, keeping the events and
/// effective edits of the publication already there. Callers retain pending
/// artifacts and report partial writes if this fails.
pub fn publish_run(
    bundle: &Path,
    id: &str,
    origin: String,
    inputs: Vec<ArtifactRevision>,
    outputs: Vec<ArtifactRevision>,
) -> Result<Publication> {
    // An earlier record of another id is a parent (D24); the run's own
    // record published again, on a repair, keeps what it holds.
    let (mut events, mut effective_edits, mut withdrawn) = Default::default();
    let (mut parents, mut parent_hashes) = (Vec::new(), BTreeMap::new());
    match latest(bundle)? {
        Some((previous, _)) if previous.id == id => {
            (events, effective_edits, withdrawn) = (
                previous.events,
                previous.effective_edits,
                previous.withdrawn,
            );
        }
        Some((previous, hash)) => {
            parent_hashes.insert(previous.id.clone(), hash);
            parents.push(previous.id);
        }
        None => {}
    }
    let publication = Publication {
        schema_version: 1,
        id: id.into(),
        session: basecraft_platform::process_instance().into(),
        parents,
        parent_hashes,
        created: basecraft_platform::now_rfc3339(),
        origin,
        inputs,
        outputs,
        events,
        effective_edits,
        withdrawn,
        completeness: Completeness {
            ancestry_known: false,
            input_bytes_available: true,
            execution_specification_available: true,
            replay_verified: false,
        },
        report_template_version: 2,
    };
    publish(bundle, &publication)?;
    Ok(publication)
}

/// The journal entry for a tool run that changed no open document, kept in
/// the run's own publication; `doc` names the operation. The record's inputs
/// are the publication's; `base` is what the run link is relative to.
pub fn file_run_entry(
    record: crate::session::ToolRunRecord<'_>,
    base: &Path,
    publication: &Publication,
    started: &str,
    actor: Actor,
    doc: &str,
) -> Result<LogEntry> {
    let names =
        |artifacts: &[ArtifactRevision]| artifacts.iter().map(|a| a.logical_name.clone()).collect();
    let op = record.op(base, names(&publication.outputs), None, None)?;
    Ok(LogEntry {
        v: basecraft_protocol::ops::LOG_VERSION,
        id: publication.id.clone(),
        t: started.into(),
        host: String::new(),
        session: publication.session.clone(),
        actor,
        doc: doc.into(),
        source: None,
        op,
        result: OpResult {
            notes: Vec::new(),
            hashes: publication
                .inputs
                .iter()
                .chain(&publication.outputs)
                .map(|artifact| (artifact.logical_name.clone(), artifact.hash.clone()))
                .collect(),
            feature: None,
        },
        state: DocState {
            counts: None,
            len: 0,
            features: 0,
            rows: None,
        },
    })
}

/// Publish a record with no parents. Artifacts must already be durable.
pub fn publish(bundle: &Path, publication: &Publication) -> Result<()> {
    stage(
        bundle,
        publication,
        publication.events.iter(),
        &publication.effective_edits,
    )?
    .publish()
    .map(|_| ())
}

/// Stage the record and `manifest.json`, which names it as the latest. They
/// sync together on publication and are renamed in order, the manifest last.
/// `events` and `effective` are the whole history, this record included.
pub(crate) fn stage<'a>(
    bundle: &Path,
    publication: &Publication,
    events: impl Iterator<Item = &'a LogEntry> + Clone,
    effective: &[basecraft_protocol::provenance::EditActivity],
) -> Result<crate::documents::PreparedWrite> {
    std::fs::create_dir_all(bundle.join("records"))?;
    link_selected_inputs(bundle, publication, events, effective)?;
    let mut prepared = crate::documents::PreparedWrite::default();
    let record = basecraft_protocol::records::encode(publication)?;
    if record.len() > 64 << 20 {
        return Err(limit("provenance record publication", 64 << 20));
    }
    prepared.push(&record_path(bundle, &publication.id)?, |w| {
        w.write_all(&record)
    })?;
    stage_json(
        &mut prepared,
        &bundle.join("manifest.json"),
        &basecraft_protocol::provenance::LatestRecord {
            schema_version: 1,
            id: publication.id.clone(),
            hash: basecraft_io::hash_bytes(&record),
        },
    )?;
    Ok(prepared)
}

/// The latest record of a saved file and the whole history it ends.
pub fn saved_history(
    result: &Path,
    project: Option<&crate::project::Project>,
) -> Result<(Publication, SavedHistory)> {
    let latest = inspect(result, project)?;
    let history = SavedHistory::read(&bundle_for_result(result, project), latest.clone())?;
    Ok((latest, history))
}

/// Write the readable reports of a saved revision into `output`, at the
/// user's request; nothing writes them on save.
pub fn write_reports(
    output: &Path,
    publication: &Publication,
    history: SavedHistory,
) -> Result<()> {
    let record =
        crate::records::document_record::DocumentRecord::from_history(publication, history);
    let mut prepared = crate::documents::PreparedWrite::default();
    std::fs::create_dir_all(output.join("reports"))?;
    let report = record.report()?;
    let mut params = String::from("activity\toperation\tparameters_json\n");
    for activity in &record.effective {
        let _ = writeln!(
            params,
            "{}\t{}\t{}",
            activity.id,
            activity.operation.name(),
            serde_json::to_string(&activity.operation)?
        );
    }
    let mappings = report
        .paragraphs
        .iter()
        .enumerate()
        .map(|(index, paragraph)| {
            serde_json::to_value(basecraft_protocol::report_records::ReportSourceMapEntry {
                paragraph: index + 1,
                activities: paragraph.activity_ids.clone(),
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?;
    for (name, bytes) in [
        ("history.md", report.history_text.as_bytes()),
        ("methods.generated.md", report.methods.as_bytes()),
        ("parameters.tsv", params.as_bytes()),
    ] {
        prepared.push(&output.join("reports").join(name), |w| w.write_all(bytes))?;
    }
    // A list carries no version; the report manifest beside it does.
    let mut map = serde_json::to_vec_pretty(&mappings)?;
    map.push(b'\n');
    prepared.push(&output.join("reports/source-map.json"), |w| {
        w.write_all(&map)
    })?;
    stage_json(
        &mut prepared,
        &output.join("reports/manifest.json"),
        &serde_json::to_value(
            basecraft_protocol::report_records::ProvenanceReportManifest {
                schema_version: 1,
                publication: publication.id.clone(),
                template_version: 2,
                output_hashes: publication.outputs.iter().map(|a| a.hash.clone()).collect(),
                redactions: vec!["machine-local paths".into(), "host".into(), "user".into()],
                citations: report.citations,
            },
        )?,
    )?;
    prepared.publish().map(|_| ())
}

/// The README of an exported history folder.
const BUNDLE_README: &[u8] = b"# Basecraft provenance v1\n\nmanifest.json names the latest record. records/ holds one immutable record per save; each names its parents, and together they are the file's history. artifacts/ holds BLAKE3-addressed original and saved bytes. Imported history before Basecraft is unknown. Checksums detect changes; they are not signatures. No file in this folder should be executed to inspect or verify it.\n";

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    if std::fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err(invalid("package bundle contains a symlink"));
    }
    std::fs::create_dir(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(invalid("package bundle contains a symlink"));
        }
        let target = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            copy_atomic(&entry.path(), &target)?;
        } else {
            return Err(invalid("package contains a special file"));
        }
    }
    Ok(())
}

/// Result and complete retained bundle copied into a new directory. This is a
/// data package, not a promise that external software can reproduce the result.
pub fn package(
    result: &Path,
    destination: &Path,
    project: Option<&crate::project::Project>,
) -> Result<()> {
    if crate::records::analysis_store::ArtifactRun::for_result(result)?.is_some()
        || bundle_for_result(result, project)
            .join("input.json")
            .is_file()
    {
        return crate::records::input_evidence::package(result, destination, project);
    }
    let issues = verify(result, project)?;
    if !issues.is_empty() {
        return Err(invalid(issues.join("; ")));
    }
    if destination.exists() {
        return Err(invalid("package destination already exists"));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-package-")
        .tempdir_in(parent)?;
    let name = result
        .file_name()
        .ok_or_else(|| invalid("result has no filename"))?;
    let copied = staging.path().join(name);
    copy_atomic(result, &copied)?;
    let publication = inspect(result, project)?;
    // The companion goes where the copy's companion is kept: the package's
    // own .basecraft.
    let companion_of_result = crate::files::companion_path(project, result);
    for artifact in publication.outputs.iter().skip(1) {
        let name = Path::new(&artifact.logical_name);
        if name
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid("unsafe output logical name"));
        }
        if companion_of_result.file_name() == Some(name.as_os_str()) {
            let packaged = crate::project::Project::document_record(None, &copied);
            crate::files::companion_folder(&packaged)?;
            copy_atomic(&companion_of_result, &packaged)?;
            continue;
        }
        copy_atomic(
            &result.parent().unwrap_or(Path::new(".")).join(name),
            &staging.path().join(name),
        )?;
    }
    let packaged_bundle = companion(&copied);
    if let Some(folder) = packaged_bundle.parent() {
        std::fs::create_dir_all(folder)?;
    }
    copy_tree(&bundle_for_result(result, project), &packaged_bundle)?;
    basecraft_io::write_atomic(&packaged_bundle.join("README.md"), |w| {
        w.write_all(BUNDLE_README)
    })?;
    // The copy carries its history beside it, outside any project.
    let copied_issues = verify(&copied, None)?;
    if !copied_issues.is_empty() {
        return Err(invalid(copied_issues.join("; ")));
    }
    // The destination is deliberately new: never merge unrelated package bytes.
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    basecraft_platform::atomic::rename_noreplace(staging.path(), destination)?;
    Ok(())
}
