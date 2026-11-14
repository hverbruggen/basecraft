//! The check before a save: which analyses used the file, and which records
//! could not be read.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_platform::files::lexical;
use basecraft_protocol::input_records::InputState;
use basecraft_protocol::provenance::InputReference;
use serde::{Deserialize, Serialize};

use super::{RunInput, index, stamp, validate_input};
use crate::{
    Error, Result,
    error::{damaged, limit},
    records::analysis_store::ArtifactRun,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavePolicy {
    #[default]
    Cancel,
    PreserveCopy,
    RemoveAnalysis,
    BreakEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffectedAnalysis {
    pub run: PathBuf,
    pub result: String,
    pub source_hash: String,
    pub can_preserve: bool,
    pub direct: bool,
    /// Backing decisions for a logical analysis; empty for a direct input record.
    /// Each child retains its own hash/direct guard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backing_analyses: Vec<AffectedAnalysis>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputConflict {
    pub target: PathBuf,
    pub analyses: Vec<AffectedAnalysis>,
    pub options: Vec<SavePolicy>,
}

/// Why a record could not be checked. Never a reason to ignore a failed
/// save or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncheckedKind {
    RegistryUnavailable,
    StoreUnavailable,
    RunUnavailable,
    LedgerInvalid,
    DependencyUnavailable,
    IdentityUnverified,
    DisplayMetadataUnavailable,
    LimitExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relevance {
    Relevant,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncheckedRecord {
    pub code: UncheckedKind,
    pub location: PathBuf,
    pub relevance: Relevance,
    /// No distance-based unrelatedness inference is made. An unreadable record
    /// can hide a direct reference, an alias, an owner or an ancestor edge.
    pub reason: String,
}

/// Advisory snapshot. It performs no registration, preservation or writes.
#[derive(Clone, Debug, Serialize)]
pub struct InputCheck {
    pub conflict: InputConflict,
    pub issues: Vec<UncheckedRecord>,
    /// Ephemeral digest of inspected inventory, including damaged bytes.
    pub inventory_revision: String,
}

impl InputCheck {
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Default)]
/// What one check has read: each record's hash (or why it could not be
/// read), the records it could not check, and each run's result name.
pub(super) struct CheckLog {
    read: BTreeMap<PathBuf, String>,
    issues: Vec<UncheckedRecord>,
    /// Each run's result name, read once per check.
    names: BTreeMap<PathBuf, String>,
}

impl CheckLog {
    pub(super) fn issue(
        &mut self,
        code: UncheckedKind,
        location: &Path,
        error: impl std::fmt::Display,
    ) {
        self.issue_for(code, Relevance::Unknown, location, error);
    }

    /// An issue for `error`, placed at the record a damaged or missing
    /// error names, so the reason does not repeat its path.
    pub(super) fn issue_from(&mut self, code: UncheckedKind, location: &Path, error: crate::Error) {
        match error {
            crate::Error::Damaged { path, detail } => self.issue(code, &path, detail),
            crate::Error::NotFound { path } => self.issue(code, &path, "was not found"),
            error => self.issue(code, location, error),
        }
    }

    pub(super) fn issue_for(
        &mut self,
        code: UncheckedKind,
        relevance: Relevance,
        location: &Path,
        error: impl std::fmt::Display,
    ) {
        self.issues.push(UncheckedRecord {
            code,
            location: location.into(),
            relevance,
            reason: error.to_string(),
        });
    }

    fn record(&mut self, path: &Path) -> Result<Vec<u8>> {
        self.record_bounded(path, 64 << 20)
    }

    pub(super) fn record_bounded(&mut self, path: &Path, mut limit: u64) -> Result<Vec<u8>> {
        let result = crate::records::provenance::read_record_with_budget(path, &mut limit);
        self.observe(path, &result);
        result
    }

    pub(super) fn observe(&mut self, path: &Path, result: &Result<Vec<u8>>) {
        self.read.insert(
            path.into(),
            match result {
                Ok(bytes) => basecraft_io::hash_bytes(bytes),
                Err(error) => format!("unavailable: {error}"),
            },
        );
    }

    fn input(&mut self, run: &Path) -> Result<RunInput> {
        // Keep the existing unsafe-record and schema validation in one owner.
        let bytes = self.record(&run.join("input.json"))?;
        validate_input(run, &bytes)
    }

    fn name(&mut self, run: &Path) -> String {
        if let Some(name) = self.names.get(run) {
            return name.clone();
        }
        let name = self.read_name(run);
        self.names.insert(run.to_path_buf(), name.clone());
        name
    }

    fn read_name(&mut self, run: &Path) -> String {
        let path = run.join("run.json");
        let result = self.record(&path).and_then(|bytes| {
            let value: serde_json::Value = basecraft_protocol::records::decode(&bytes)?;
            Ok(value["result"]
                .as_str()
                .unwrap_or("Unpublished analysis")
                .to_owned())
        });
        match result {
            Ok(name) => name,
            Err(error) => {
                self.issue_for(
                    UncheckedKind::DisplayMetadataUnavailable,
                    Relevance::Relevant,
                    &path,
                    error,
                );
                "Unpublished analysis".into()
            }
        }
    }
}

impl std::fmt::Display for InputConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // CLI/SDK callers receive a structured conflict without an interactive prompt.
        write!(
            f,
            "analysis_input_conflict: {}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}

pub(super) fn same_source(run: &Path, reference: &InputReference, target: &Path) -> bool {
    let source = run.join(&reference.path);
    if source.canonicalize().ok().as_deref() == Some(target) {
        return true;
    }
    if let (Ok(a), Ok(b)) = (stamp(&source), stamp(target)) {
        return a.device.is_some() && a.device == b.device && a.inode == b.inode;
    }
    // Missing inputs still match their lexical old location.
    lexical(&source) == lexical(target)
}

/// Metadata-only UI query. Matching metadata is a signal, never a hash verdict.
pub fn conflicts(target: &Path) -> Result<InputConflict> {
    conservative(check(target)?)
}

fn conservative(check: InputCheck) -> Result<InputConflict> {
    if !check.is_complete() {
        return Err(crate::Error::InputCheckIncomplete(Box::new(check)));
    }
    Ok(check.conflict)
}

/// Check which analyses used `target`, writing nothing; callers check again
/// under `lock` before acting. A failed lock or write is an error, never an
/// unchecked record.
pub fn check(target: &Path) -> Result<InputCheck> {
    check_for(target, None)
}

/// As [`check`], for a session that holds `project`: a lost index of a folder
/// that the project's root store names makes the check incomplete.
pub(crate) fn check_for(target: &Path, project: Option<&Path>) -> Result<InputCheck> {
    let (canonical, selected, log) = selection(target, project)?;
    inspect(&canonical, &selected, log)
}

pub(super) fn selection(
    target: &Path,
    project: Option<&Path>,
) -> Result<(PathBuf, Vec<PathBuf>, CheckLog)> {
    let mut log = CheckLog::default();
    let absolute = std::path::absolute(target)?;
    let canonical = absolute.canonicalize().unwrap_or_else(|_| absolute.clone());
    let selected = index::select(&canonical, &absolute, project, &mut log);
    Ok((canonical, selected, log))
}

#[derive(Default)]
struct StoreScan {
    runs: Vec<(PathBuf, RunInput)>,
    analyses: Vec<AffectedAnalysis>,
    metadata_bytes: u64,
}

fn scan_store(store: &Path, target: &Path, scan: &mut StoreScan, log: &mut CheckLog) {
    basecraft_io::counters::record_store_inspection(store);
    // The marker can be damaged in different ways with the same parse error.
    // Its observed bytes participate in consent, even when validation fails.
    let _ = log.record_bounded(&store.join("store.json"), 4096);
    let result = ArtifactRun::validate_store(store)
        .and_then(|()| Ok(std::fs::read_dir(store.join("runs"))?));
    let entries = match result {
        Ok(entries) => entries,
        Err(error) => {
            log.issue(UncheckedKind::StoreUnavailable, store, error);
            return;
        }
    };
    for (count, entry) in entries.enumerate() {
        if count >= 4096 {
            log.issue(
                UncheckedKind::LimitExceeded,
                store,
                "store exceeds 4096 runs; explicit inspection required",
            );
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log.issue(UncheckedKind::RunUnavailable, &store.join("runs"), error);
                continue;
            }
        };
        let run = entry.path();
        match scan_run(&entry, target, &mut scan.metadata_bytes, log) {
            Ok(Some((input, analysis, bytes))) => {
                scan.metadata_bytes = bytes;
                scan.runs.push((run, input));
                scan.analyses.extend(analysis);
            }
            Ok(None) => {}
            Err(error) => {
                log.issue_from(UncheckedKind::LedgerInvalid, &run.join("input.json"), error)
            }
        }
    }
}

type RunScan = (RunInput, Option<AffectedAnalysis>, u64);

fn scan_run(
    entry: &std::fs::DirEntry,
    target: &Path,
    metadata_bytes: &mut u64,
    log: &mut CheckLog,
) -> Result<Option<RunScan>> {
    let kind = entry.file_type()?;
    if kind.is_file() {
        return Ok(None);
    }
    if !kind.is_dir() {
        return Err(crate::Error::Damaged {
            path: entry.path(),
            detail: "unsafe entry in analysis store".into(),
        });
    }
    let run = entry.path();
    if !run.join("input.json").try_exists()? {
        return Ok(None); // Reserved, no input registered yet.
    }
    let bytes = metadata_bytes.saturating_add(std::fs::metadata(run.join("input.json"))?.len());
    *metadata_bytes = bytes;
    if bytes > 64 << 20 {
        log.issue(
            UncheckedKind::LimitExceeded,
            &run.join("input.json"),
            "known analysis metadata exceeds 64 MiB query limit",
        );
        return Ok(None);
    }
    let input = log.input(&run)?;
    let mut analysis = None;
    let proposal_target = input.proposal_target == Some(true);
    let source_matches = if input.state == InputState::Referenced && !proposal_target {
        input.source.as_ref().is_some_and(|reference| {
            if same_source(&run, reference, target) {
                return true;
            }
            let source = run.join(&reference.path);
            if let Err(error) = source
                .canonicalize()
                .map_err(crate::Error::from)
                .and_then(|_| stamp(&source))
            {
                log.issue(UncheckedKind::IdentityUnverified, &source, error);
            }
            false
        })
    } else {
        false
    };
    if source_matches && let Some(reference) = &input.source {
        let can_preserve = stamp(target).is_ok_and(|s| {
            s.size == reference.stamp.size
                && s.modified_seconds == reference.stamp.modified_seconds
                && s.modified_nanos == reference.stamp.modified_nanos
        });
        analysis = Some(AffectedAnalysis {
            result: log.name(&run),
            run,
            source_hash: reference.hash.clone(),
            can_preserve,
            direct: true,
            backing_analyses: vec![],
        });
    }
    Ok(Some((input, analysis, bytes)))
}

pub fn conflicts_in(target: &Path, stores: &[PathBuf]) -> Result<InputConflict> {
    conservative(check_in(target, stores)?)
}

pub fn check_in(target: &Path, stores: &[PathBuf]) -> Result<InputCheck> {
    inspect(target, stores, CheckLog::default())
}

pub(super) fn inspect(target: &Path, stores: &[PathBuf], mut log: CheckLog) -> Result<InputCheck> {
    let mut scan = StoreScan::default();
    for store in stores {
        scan_store(store, target, &mut scan, &mut log);
    }
    let mut analyses = scan.analyses;
    let mut known_runs = scan.runs;
    known_runs.sort_by(|a, b| a.0.cmp(&b.0));
    analyses.sort_by(|a, b| a.run.cmp(&b.run));
    // Metadata-only transitive closure: preserving the direct original repairs
    // descendants through their owning input record, while removal lists every record
    // that would otherwise be left with a missing ancestor.
    loop {
        let before = analyses.len();
        for (run, input) in &known_runs {
            if analyses.iter().any(|a| &a.run == run) {
                continue;
            }
            let mut dependent = false;
            for parent in &input.upstream {
                let path = &parent.bundle;
                match depends_on_inspected(&run.join(path), &analyses, &mut log) {
                    Ok(found) => dependent |= found,
                    Err(error) => {
                        log.issue_from(UncheckedKind::DependencyUnavailable, &run.join(path), error)
                    }
                }
            }
            if dependent {
                analyses.push(AffectedAnalysis {
                    run: run.clone(),
                    result: log.name(run),
                    source_hash: String::new(),
                    can_preserve: true,
                    direct: false,
                    backing_analyses: vec![],
                });
            }
        }
        if before == analyses.len() {
            break;
        }
    }
    // Only explicit ownership collapses records into one analysis. Equal
    // names/hashes alone never merge distinct analyses.
    let mut owners = BTreeMap::new();
    for (run, input) in &known_runs {
        if let Some(owner) = input.analysis_run.as_deref() {
            let owner = lexical(&run.join(owner));
            if known_runs.iter().any(|(path, _)| lexical(path) == owner) {
                owners.insert(run.clone(), owner);
            } else {
                log.issue(
                    UncheckedKind::DependencyUnavailable,
                    &owner,
                    "analysis owner is unavailable in inspected inventory",
                );
            }
        }
    }
    let mut groups: BTreeMap<PathBuf, Vec<AffectedAnalysis>> = BTreeMap::new();
    for analysis in analyses {
        let owner = owners
            .get(&analysis.run)
            .cloned()
            .unwrap_or_else(|| analysis.run.clone());
        groups.entry(owner).or_default().push(analysis);
    }
    // A logical analysis owns all its dedicated input records, including inputs other
    // than the overwritten file. Break/remove apply to that whole inventory;
    // preserve only touches the direct references marked above.
    for (run, _) in &known_runs {
        let owner = owners.get(run).unwrap_or(run);
        if let Some(backing) = groups.get_mut(owner)
            && !backing.iter().any(|analysis| &analysis.run == run)
        {
            backing.push(AffectedAnalysis {
                run: run.clone(),
                result: log.name(run),
                source_hash: String::new(),
                can_preserve: true,
                direct: false,
                backing_analyses: vec![],
            });
        }
    }
    let analyses: Vec<_> = groups
        .into_iter()
        .map(|(owner, mut backing)| {
            backing.sort_by_key(|a| a.run != owner);
            Ok(AffectedAnalysis {
                result: log.name(&owner),
                run: owner,
                source_hash: backing
                    .iter()
                    .find(|a| a.direct)
                    .map_or_else(String::new, |a| a.source_hash.clone()),
                can_preserve: backing.iter().all(|a| a.can_preserve),
                direct: backing.iter().any(|a| a.direct),
                backing_analyses: backing,
            })
        })
        .collect::<Result<_>>()?;
    let mut options = vec![
        SavePolicy::Cancel,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ];
    if !analyses.is_empty() && analyses.iter().all(|a| a.can_preserve) {
        options.insert(1, SavePolicy::PreserveCopy);
    }
    let conflict = InputConflict {
        target: target.into(),
        analyses,
        options,
    };
    log.issues
        .sort_by(|a, b| a.location.cmp(&b.location).then(a.reason.cmp(&b.reason)));
    log.issues.dedup();
    let inventory_revision = basecraft_io::hash_bytes(&serde_json::to_vec(&(
        stores,
        &conflict,
        &known_runs,
        &log.read,
        &log.issues,
    ))?);
    Ok(InputCheck {
        conflict,
        issues: log.issues,
        inventory_revision,
    })
}

// Save As companions are not store runs. Include each delegation location
// before reading it, so a removed owner can still be matched by its old path.
fn depends_on_inspected(
    bundle: &Path,
    analyses: &[AffectedAnalysis],
    log: &mut CheckLog,
) -> Result<bool> {
    let mut location = lexical(bundle);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..32 {
        if analyses.iter().any(|a| lexical(&a.run) == location) {
            return Ok(true);
        }
        if !seen.insert(location.clone()) {
            return Err(damaged(location, "input evidence contains a cycle"));
        }
        if !location.join("input.json").try_exists()? {
            return Err(Error::NotFound {
                path: location.join("input.json"),
            });
        }
        let input = log.input(&location)?;
        let Some(origin) = &input.origin_run else {
            return Ok(false);
        };
        location = lexical(&location.join(origin));
    }
    Err(limit("input evidence ancestry", 32))
}

#[cfg(test)]
mod store_scan_tests {
    use super::super::{relative, write_input, write_json};
    use super::*;

    fn fixture(root: &Path, source: &Path) -> ArtifactRun {
        let run = ArtifactRun::create(None, &root.join("species.spart"), "scan-test").unwrap();
        write_input(
            run.directory(),
            &RunInput {
                schema_version: 1,
                source: Some(InputReference {
                    path: relative(run.directory(), source).unwrap(),
                    stamp: stamp(source).unwrap(),
                    hash: basecraft_io::hash_file(source).unwrap(),
                    extra: BTreeMap::new(),
                }),
                submitted_hash: basecraft_io::hash_file(source).unwrap(),
                submitted_size: stamp(source).unwrap().size,
                transformation: basecraft_protocol::input_records::Transformation::Identity,
                state: InputState::Referenced,
                analysis_run: None,
                proposal_target: None,
                upstream: Vec::new(),
                origin_run: None,
                extra: BTreeMap::new(),
            },
        )
        .unwrap();
        write_json(
            &run.artifact("run.json").unwrap(),
            &serde_json::json!({"result":"species.spart"}),
        )
        .unwrap();
        run
    }

    #[test]
    fn a_damaged_ledger_is_an_issue_at_its_path_without_repeating_it() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let ledger = run.directory().join("input.json");
        std::fs::write(&ledger, b"{").unwrap();
        let check = check_in(&source, &[run.store().into()]).unwrap();
        let issue = &check.issues[0];
        assert_eq!(issue.code, UncheckedKind::LedgerInvalid);
        assert_eq!(issue.location, ledger);
        assert!(
            !issue.reason.contains(&*root.path().to_string_lossy()),
            "{}",
            issue.reason
        );
    }

    #[test]
    fn disappearing_store_keeps_prior_evidence_and_marks_uncertainty() {
        for delete in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("input.nwk");
            std::fs::write(&source, "(a:1,b:1);").unwrap();
            let run = fixture(root.path(), &source);
            let store = run.directory().parent().unwrap().parent().unwrap();
            let mut scan = StoreScan::default();
            let mut log = CheckLog::default();
            scan_store(store, &source, &mut scan, &mut log);
            assert_eq!(scan.analyses.len(), 1);
            if delete {
                std::fs::remove_dir_all(store).unwrap();
            } else {
                std::fs::rename(store, root.path().join("moved-store")).unwrap();
            }
            // The store changes between two scans of one check.
            // Never discard successfully inspected evidence on a later failure.
            scan_store(store, &source, &mut scan, &mut log);
            assert_eq!(scan.analyses.len(), 1);
            assert_eq!(log.issues[0].code, UncheckedKind::StoreUnavailable);
            let check = check_in(&source, &[store.into()]).unwrap();
            assert!(!check.is_complete());
            assert!(conflicts_in(&source, &[store.into()]).is_err());
        }
    }

    #[test]
    fn missing_store_does_not_discard_other_store_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let store = run.directory().parent().unwrap().parent().unwrap();
        let check = check_in(&source, &[root.path().join("missing"), store.into()]).unwrap();
        assert!(!check.is_complete());
        assert_eq!(check.conflict.analyses.len(), 1);
        assert_eq!(check.conflict.analyses[0].run, run.directory());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_run_symlinks_are_not_treated_as_disappeared_directories() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let runs = run.directory().parent().unwrap();
        let store = runs.parent().unwrap();
        std::os::unix::fs::symlink(root.path().join("missing"), runs.join("unsafe")).unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
    }

    #[test]
    fn present_store_with_damaged_metadata_still_refuses_save() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let store = run.directory().parent().unwrap().parent().unwrap();
        // A registered reference cannot silently lose its run details.
        std::fs::remove_file(run.artifact("run.json").unwrap()).unwrap();
        assert!(matches!(
            conflicts_in(&source, &[store.into()]),
            Err(crate::Error::InputCheckIncomplete(_))
        ));
        std::fs::write(run.artifact("input.json").unwrap(), "damaged").unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
        std::fs::remove_file(store.join("store.json")).unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
        assert!(store.is_dir());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "(a:1,b:1);");
    }
}
