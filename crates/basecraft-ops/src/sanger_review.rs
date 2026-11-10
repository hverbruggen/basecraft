//! Replayable review decisions for a bounded Sanger consensus draft.
//! The source draft and AB1 files remain immutable; no reference base is
//! inferred or substituted by these operations.

mod edit;
mod save;
mod storage;
pub use storage::{SangerRun, record_addition};
pub(crate) use storage::{is_review, relocate_package};

use std::path::{Path, PathBuf};

use basecraft_docs::sanger::{ReviewReference, ReviewSettings, SangerDraft};
use serde::Serialize;

/// Recognised saved Sanger evidence, determined from validated content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceKind {
    Draft,
    Review,
}

/// Inspect JSON without creating records or treating arbitrary JSON as a trace.
pub fn evidence_kind(path: &Path) -> Option<EvidenceKind> {
    if !path.extension()?.eq_ignore_ascii_case("json") {
        return None;
    }
    let bytes = crate::provenance::read_record(path).ok()?;
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && value.get("version").is_some()
    {
        return SangerReviewSession::decode(&bytes, path)
            .ok()
            .map(|_| EvidenceKind::Review);
    }
    let draft: SangerDraft = serde_json::from_slice(&bytes).ok()?;
    draft.validate().ok()?;
    Some(EvidenceKind::Draft)
}

/// Read a saved review for headless inspection, recognised by content as in
/// [`evidence_kind`]. Any other `.json` file is refused with the reason, so a
/// damaged, unknown or future review is never mistaken for another format.
/// `None`: not JSON, so not Sanger evidence. Creates no records.
pub fn saved_review(path: &Path) -> Option<Result<SangerReviewSession, String>> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return None;
    }
    let name = path.display();
    Some(match evidence_kind(path) {
        Some(EvidenceKind::Review) => SangerReviewSession::load(path),
        Some(EvidenceKind::Draft) => Err(format!(
            "{name} is an unreviewed Sanger draft, not a saved Sanger review; start its review in Basecraft"
        )),
        None => Err(match SangerReviewSession::load(path) {
            Err(error) => format!("{name} is not a readable saved Sanger review: {error}"),
            Ok(_) => format!("{name} is not a readable saved Sanger review"),
        }),
    })
}

/// Write a review recovered from application state to `destination`, a new
/// folder, with every retained call set. The original checkpoint is unchanged.
/// Failed saves leave the destination and any partial output in place.
pub fn recover(checkpoint: &Path, destination: &Path) -> crate::Result<PathBuf> {
    recover_with_saver(checkpoint, destination, |review, target| {
        review.save_with_evidence(target, crate::input_evidence::SavePolicy::Cancel)
    })
}

fn recover_with_saver(
    checkpoint: &Path,
    destination: &Path,
    saver: impl FnOnce(&mut SangerReviewSession, &Path) -> Result<(), String>,
) -> crate::Result<PathBuf> {
    use crate::error::invalid;
    let review = SangerReviewSession::load(checkpoint).map_err(invalid)?;
    let name = review
        .recovery_path()
        .and(review.storage_anchor().file_name())
        .ok_or_else(|| invalid("not a Sanger review recovery checkpoint"))?
        .to_owned();
    // Refuses an existing destination, including a concurrent recovery.
    std::fs::create_dir(destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            invalid("recovery destination already exists")
        } else {
            error.into()
        }
    })?;
    let target = destination.join(name);
    // A continuation owns a separate checkpoint, even when saving fails.
    let mut review = review.fork_for_operation();
    saver(&mut review, &target).map_err(invalid)?;
    Ok(target)
}

pub use basecraft_protocol::sanger::{MaskSettings, ReviewChoice, ReviewOp};

#[derive(Clone, Debug, Serialize)]
struct Change {
    columns: Vec<usize>,
    before: Vec<ReviewChoice>,
    after: Vec<ReviewChoice>,
    manual_before: Vec<bool>,
    manual_after: Vec<bool>,
    triage: Option<(
        ReviewSettings,
        Option<ReviewReference>,
        ReviewSettings,
        Option<ReviewReference>,
    )>,
    mask: Option<(MaskSettings, MaskSettings)>,
}

use basecraft_protocol::sanger_records::{
    SavedCallSets as StoredCallSets, SavedReview as StoredReview,
};
use storage::EvidencePaths;

pub struct SangerReviewSession {
    save_authority: Option<save::RootAuthority>,
    // Only the selected session owns the inactive sets. Each set owns its replay history.
    retained: Vec<SangerReviewSession>,
    pub draft: SangerDraft,
    pub draft_path: PathBuf,
    pub draft_sha256: String,
    draft_json: String,
    decisions: Vec<ReviewChoice>,
    pub mask: MaskSettings,
    pub settings: ReviewSettings,
    pub reference: Option<ReviewReference>,
    manual: Vec<bool>,
    events: Vec<ReviewOp>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    evidence: storage::ReviewEvidence,
    anchor: PathBuf,
    recovery: Option<PathBuf>,
    accepted: Option<crate::save_guard::Revision>,
    stored_extra: std::collections::BTreeMap<String, serde_json::Value>,
}

impl SangerReviewSession {
    fn default_mask(length: usize) -> MaskSettings {
        MaskSettings {
            start: 0,
            end: length,
            forward: String::new(),
            reverse: String::new(),
            minimum_overlap: 12,
            error_percent: 25,
            evidence: String::new(),
        }
    }
    /// Describe the actual review state, retaining the complete decision audit separately.
    pub fn document_report(&self, unsaved: bool) -> Result<crate::document_record::Report, String> {
        use crate::document_record::{DocumentRecord, HistoryRow, MethodsParagraph};
        let mut state = serde_json::json!({
            "draft": self.draft, "draft_sha256": self.draft_sha256, "events": self.events,
            "settings": self.settings, "reference": self.reference, "mask": self.mask,
            "decisions": self.decisions, "manual": self.manual,
        });
        // Other call sets of a multi-set review are part of its revision; a
        // single-set (v1) report is unchanged.
        let retained: Vec<_> = self
            .retained
            .iter()
            .map(|set| {
                serde_json::json!({"draft_sha256": set.draft_sha256, "engine": set.draft.engine,
                    "version": set.draft.version_output.trim(), "events": set.events,
                    "decisions": set.decisions, "manual": set.manual, "mask": set.mask,
                    "unresolved": set.unresolved()})
            })
            .collect();
        if !retained.is_empty() {
            state["call_sets"] = serde_json::Value::Array(retained.clone());
        }
        let raw = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
        let mut record = DocumentRecord::empty(&self.draft_path.display().to_string());
        record.revision = basecraft_io::hash_bytes(raw.as_bytes());
        record.unsaved = unsaved;
        let mut report = record.report().map_err(|e| e.to_string())?;
        report.raw = raw;
        for (index, event) in self.events.iter().enumerate() {
            let summary = match event {
                ReviewOp::AutoAccept { columns } => {
                    format!("Automatically accepted {} review columns", columns.len())
                }
                ReviewOp::Evaluate { .. } => {
                    "Evaluated automatic calls with recorded review settings".into()
                }
                ReviewOp::Set { columns, choice } => {
                    format!("Set {} review columns to {choice:?}", columns.len())
                }
                ReviewOp::Mask { settings } => format!(
                    "Retained review columns {}–{}",
                    settings.start.saturating_add(1),
                    settings.end
                ),
                ReviewOp::Undo => "Undo review decision".into(),
                ReviewOp::Redo => "Redo review decision".into(),
                ReviewOp::Export { .. } => "Exported reviewed consensus".into(),
            };
            report.history.push(HistoryRow {
                id: format!("review:{index}"),
                time: String::new(),
                summary,
                status: "Review audit".into(),
                contributing: false,
                details: serde_json::to_string_pretty(event).map_err(|e| e.to_string())?,
            });
        }
        for (set, details) in self.retained.iter().zip(&retained) {
            report.history.push(HistoryRow {
                id: format!("review:call-set:{}", set.draft_sha256),
                time: String::new(),
                summary: format!(
                    "Retained call set from {}: {} columns unresolved",
                    set.draft.engine,
                    set.unresolved()
                ),
                status: "Retained call set".into(),
                contributing: false,
                details: serde_json::to_string_pretty(details).map_err(|e| e.to_string())?,
            });
        }
        let manual = self.manual.iter().filter(|m| **m).count();
        let mut text = format!(
            "Sanger consensus evidence was prepared using {} (recorded version: {}). The current review retains columns {}–{} and contains {manual} manually reviewed columns; {} columns remain unresolved. Review thresholds and primer/masking settings are retained in the supporting record. Earlier laboratory processing is not recorded.",
            self.draft.engine,
            self.draft.version_output.trim(),
            self.mask.start.saturating_add(1),
            self.mask.end,
            self.unresolved()
        );
        if !self.retained.is_empty() {
            text = format!(
                "{text} Other retained call sets: {}; each keeps its own decisions, and this report describes the selected set.",
                self.retained.len()
            );
        }
        report.paragraphs = vec![MethodsParagraph {
            activity_ids: vec!["review:state".into()],
            text: text.clone(),
            parameters: report.raw.clone(),
        }];
        report.history.insert(
            0,
            HistoryRow {
                id: "review:state".into(),
                time: String::new(),
                summary: "Current Sanger review decisions and settings".into(),
                status: "Contributing".into(),
                contributing: true,
                details: report.raw.clone(),
            },
        );
        report.methods = text;
        report.history_text = report
            .history
            .iter()
            .map(|r| r.summary.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(report)
    }

    pub fn masked(&self, column: usize) -> bool {
        column < self.mask.start || column >= self.mask.end
    }

    pub fn new(draft_path: &Path) -> Result<Self, String> {
        let draft_json = std::fs::read_to_string(draft_path).map_err(|e| e.to_string())?;
        if draft_json.len() > 32 * 1024 * 1024 {
            return Err("Sanger evidence exceeds 32 MiB".into());
        }
        let draft: SangerDraft = serde_json::from_str(&draft_json).map_err(|e| e.to_string())?;
        draft.validate()?;
        let mut session = Self::from_draft(draft, draft_path)?;
        session.draft_sha256 = basecraft_io::sha256_bytes(draft_json.as_bytes());
        session.draft_json = draft_json;
        Ok(session)
    }

    /// Start an unsaved review without creating files or invoking a caller.
    pub fn from_draft(draft: SangerDraft, draft_path: &Path) -> Result<Self, String> {
        draft.validate()?;
        let draft_json = serde_json::to_string(&draft).map_err(|e| e.to_string())?;
        let hash = basecraft_io::sha256_bytes(draft_json.as_bytes());
        let decisions = vec![ReviewChoice::Pending; draft.columns.len()];
        Ok(Self {
            save_authority: Some(save::RootAuthority::new()),
            retained: Vec::new(),
            mask: Self::default_mask(draft.columns.len()),
            settings: ReviewSettings::default(),
            reference: None,
            manual: vec![false; draft.columns.len()],
            draft,
            draft_path: draft_path.to_path_buf(),
            draft_sha256: hash,
            draft_json,
            decisions,
            events: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            evidence: storage::ReviewEvidence::default(),
            anchor: std::path::absolute(draft_path).map_err(|e| e.to_string())?,
            recovery: None,
            accepted: None,
            stored_extra: std::collections::BTreeMap::new(),
        })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("Sanger review exceeds 64 MiB".into());
        }
        let mut session = Self::decode(&bytes, path)?;
        if session.recovery.is_none() {
            session.accepted = Some(
                crate::save_guard::Revision::accepted(path, &bytes).map_err(|e| e.to_string())?,
            );
        }
        Ok(session)
    }

    fn decode(bytes: &[u8], path: &Path) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        match value["version"].as_u64() {
            Some(1) => Self::from_stored(
                serde_json::from_value(value).map_err(|e| e.to_string())?,
                path,
            ),
            Some(2) => {
                let stored: StoredCallSets =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                if stored.sets.is_empty() || stored.sets.len() > 32 {
                    return Err("Invalid number of retained Sanger call sets".into());
                }
                let mut sets = Vec::new();
                let mut ids = std::collections::BTreeSet::new();
                for set in stored.sets {
                    let session = Self::from_stored(set, path)?;
                    if !ids.insert(session.draft_sha256.clone()) {
                        return Err("Duplicate retained Sanger call set".into());
                    }
                    sets.push(session);
                }
                let selected = sets
                    .iter()
                    .position(|set| set.draft_sha256 == stored.selected)
                    .ok_or("Selected Sanger call set is missing")?;
                let mut session = sets.remove(selected);
                if session.draft.inputs.len() != 1
                    || sets.iter().any(|set| {
                        set.draft.inputs.len() != 1
                            || set.draft.inputs[0].sha256 != session.draft.inputs[0].sha256
                    })
                {
                    return Err("Retained call sets refer to different Sanger sources".into());
                }
                for set in &mut sets {
                    set.recovery = None;
                    set.demote_save_authority();
                }
                session.retained = sets;
                Ok(session)
            }
            _ => Err("Unsupported Sanger review version".into()),
        }
    }

    fn from_stored(stored: StoredReview, path: &Path) -> Result<Self, String> {
        if stored.version != 1 {
            return Err("Unsupported Sanger review version".into());
        }
        if basecraft_io::sha256_bytes(stored.draft_json.as_bytes()) != stored.draft_sha256 {
            return Err("Embedded source draft changed since review".into());
        }
        let draft: SangerDraft =
            serde_json::from_str(&stored.draft_json).map_err(|e| e.to_string())?;
        draft.validate()?;
        let decisions = vec![ReviewChoice::Pending; draft.columns.len()];
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let mut evidence = stored.evidence;
        if evidence.schema_version != 1 {
            return Err("Unsupported Sanger support version".into());
        }
        evidence
            .map_paths(|p| Ok(parent.join(p)))
            .map_err(|e| e.to_string())?;
        let bundle = crate::provenance::bundle_for_result(path);
        if bundle.join("manifest.json").is_file() && !evidence.runs.contains(&bundle) {
            evidence.runs.push(bundle);
        }
        let mut session = Self {
            save_authority: Some(save::RootAuthority::new()),
            retained: Vec::new(),
            mask: Self::default_mask(draft.columns.len()),
            settings: ReviewSettings::default(),
            reference: None,
            manual: vec![false; draft.columns.len()],
            draft,
            draft_path: stored.draft_path,
            draft_sha256: stored.draft_sha256,
            draft_json: stored.draft_json,
            decisions,
            events: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            evidence,
            anchor: std::path::absolute(path).map_err(|e| e.to_string())?,
            recovery: None,
            accepted: None,
            stored_extra: stored.extra,
        };
        for event in stored.events {
            if let ReviewOp::Export { sha256, .. } = &event {
                // An export is historical evidence, not an input to review
                // decisions. Moving/removing that FASTA must not make the
                // embedded review unrecoverable. Live apply still checks bytes.
                if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("Invalid exported consensus SHA-256".into());
                }
                session.events.push(event);
            } else {
                session.apply(event)?;
            }
        }
        session.resume_recovery(path)?;
        Ok(session)
    }

    /// Draft hashes bind source bytes, producer identity/settings and coordinates.
    pub fn call_sets(&self) -> Vec<(&str, &str)> {
        std::iter::once(self)
            .chain(self.retained.iter())
            .map(|set| (set.draft_sha256.as_str(), set.draft.engine.as_str()))
            .collect()
    }

    /// Keep older computed sets and their decisions when a new producer is used.
    pub fn retain_call_set(&mut self, mut session: Self) -> Result<(), String> {
        self.invalidate_save_authority();
        session.demote_save_authority();
        if !session.retained.is_empty() || self.retained.len() >= 31 {
            return Err("Too many or nested Sanger call sets".into());
        }
        if self.draft.inputs.len() != 1
            || session.draft.inputs.len() != 1
            || !self.draft.inputs[0]
                .sha256
                .eq_ignore_ascii_case(&session.draft.inputs[0].sha256)
        {
            return Err("Sanger call sets refer to different source bytes".into());
        }
        if !self
            .call_sets()
            .iter()
            .any(|(id, _)| *id == session.draft_sha256)
        {
            self.retained.push(session);
        }
        self.checkpoint()
    }

    pub fn select_call_set(&mut self, id: &str) -> Result<(), String> {
        self.invalidate_save_authority();
        let previous = self.draft_sha256.clone();
        self.swap_call_set(id)?;
        if let Err(error) = self.checkpoint() {
            self.swap_call_set(&previous)?;
            return Err(error);
        }
        Ok(())
    }

    fn swap_call_set(&mut self, id: &str) -> Result<(), String> {
        if self.draft_sha256 == id {
            return Ok(());
        }
        let index = self
            .retained
            .iter()
            .position(|set| set.draft_sha256 == id)
            .ok_or("Unknown Sanger call set")?;
        let mut retained = std::mem::take(&mut self.retained);
        let mut selected = retained.remove(index);
        std::mem::swap(self, &mut selected);
        // Recovery and save destination belong to the whole review document.
        std::mem::swap(&mut self.save_authority, &mut selected.save_authority);
        std::mem::swap(&mut self.recovery, &mut selected.recovery);
        std::mem::swap(&mut self.anchor, &mut selected.anchor);
        std::mem::swap(&mut self.accepted, &mut selected.accepted);
        retained.push(selected);
        self.retained = retained;
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.save_with_evidence(path, crate::input_evidence::SavePolicy::Cancel)
    }

    /// Explicit referenced-input policy, shared with ordinary document saves.
    pub fn save_with_evidence(
        &mut self,
        path: &Path,
        policy: crate::input_evidence::SavePolicy,
    ) -> Result<(), String> {
        self.invalidate_save_authority();
        self.save_durable(path, policy).map_err(|e| e.to_string())
    }

    pub fn decision(&self, column: usize) -> Option<&ReviewChoice> {
        self.decisions.get(column)
    }

    pub fn unresolved(&self) -> usize {
        self.decisions
            .iter()
            .enumerate()
            .filter(|(i, choice)| {
                !self.masked(*i) && matches!(choice, ReviewChoice::Pending | ReviewChoice::Reject)
            })
            .count()
    }

    pub fn resolved_sequence(&self) -> Option<String> {
        if self.unresolved() > 0 {
            return None;
        }
        Some(
            self.decisions
                .iter()
                .zip(&self.draft.columns)
                .enumerate()
                .filter_map(|(i, (choice, column))| {
                    if self.masked(i) {
                        return None;
                    }
                    let base = match choice {
                        ReviewChoice::Accept => column.consensus,
                        ReviewChoice::Edit(base) | ReviewChoice::Auto(base) => *base,
                        ReviewChoice::Pending | ReviewChoice::Reject => return None,
                    };
                    (base != '-').then_some(base)
                })
                .collect(),
        )
    }

    pub fn apply(&mut self, op: ReviewOp) -> Result<(), String> {
        self.invalidate_save_authority();
        match &op {
            ReviewOp::Undo => {
                let change = self.undo.pop().ok_or("Nothing to undo")?;
                self.install_change(&change, false);
                self.redo.push(change);
            }
            ReviewOp::Redo => {
                let change = self.redo.pop().ok_or("Nothing to redo")?;
                self.install_change(&change, true);
                self.undo.push(change);
            }
            ReviewOp::Export { path, sha256 } => {
                let actual = basecraft_io::sha256_file(path).map_err(|e| e.to_string())?;
                if actual != *sha256 {
                    return Err("Exported consensus changed since review".into());
                }
            }
            _ => {
                let change = self.prepare_change(&op)?;
                self.install_change(&change, true);
                // Initial triage stays outside legacy undo history.
                if !matches!(op, ReviewOp::AutoAccept { .. }) {
                    if !matches!(op, ReviewOp::Evaluate { .. }) || !self.events.is_empty() {
                        self.undo.push(change);
                    }
                    self.redo.clear();
                }
            }
        }
        self.events.push(op);
        self.checkpoint()
    }

    /// Export only a fully resolved specimen sequence. The output hash is a
    /// replayable operation; source AB1 and draft paths can never be replaced.
    pub fn export_consensus(&mut self, path: &Path, label: &str) -> Result<(), String> {
        self.invalidate_save_authority();
        let sequence = self
            .resolved_sequence()
            .ok_or("Resolve every review column before export")?;
        if sequence.is_empty() {
            return Err("Resolved consensus is empty".into());
        }
        if path.exists()
            || path == self.draft_path
            || self.draft.inputs.iter().any(|read| read.path == path)
        {
            return Err("Choose a new output file distinct from the source traces".into());
        }
        let label: String = label
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if label.is_empty() {
            return Err("Give the specimen a name".into());
        }
        let fasta = format!(">{label}\n{sequence}\n");
        self.start_recovery()?;
        // Same-filesystem no-clobber promotion protects a file created after
        // preflight; do not replace another writer's output.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        std::io::Write::write_all(staged.as_file_mut(), fasta.as_bytes())
            .map_err(|e| e.to_string())?;
        staged.as_file().sync_all().map_err(|e| e.to_string())?;
        staged.persist_noclobber(path).map_err(|e| e.to_string())?;
        let sha256 = basecraft_io::sha256_file(path).map_err(|e| e.to_string())?;
        self.apply(ReviewOp::Export {
            path: path.to_path_buf(),
            sha256,
        })?;
        self.publish_consensus(path).map_err(|e| format!("Consensus written to {}; supporting publication failed: {e}. Review decisions remain recoverable.",path.display()))
    }

    /// Initial automatic triage can be regenerated when orienting a fresh draft.
    pub fn automatic_only(&self) -> bool {
        self.events.iter().all(|event| {
            matches!(
                event,
                ReviewOp::AutoAccept { .. } | ReviewOp::Evaluate { .. }
            )
        })
    }

    pub fn events(&self) -> &[ReviewOp] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_mask_excludes_pending_bases_and_replays_with_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("source.json");
        let columns: Vec<_> = "ACGT".chars().enumerate().map(|(i,b)| serde_json::json!({"column": i+1,"consensus":b,"support":[{"read":1,"base":b,"kind":"call"}],"conflict":false,"review":true})).collect();
        let data = serde_json::json!({"status":"draft_requires_review","engine":"Tracy","version_output":"test","engine_sha256":"a".repeat(64),"mode":"basecall","inputs":[{"read":1,"path":"x.ab1","sha256":"b".repeat(64),"orientation":"forward"}],"consensus":"ACGT","consensus_length":4,"minimum_overlap":1,"minimum_identity":0.8,"overlaps":[],"aligned_columns":4,"conflict_columns":0,"columns":columns});
        std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let mut session = SangerReviewSession::new(&path).unwrap();
        session
            .apply(ReviewOp::Set {
                columns: vec![1, 2],
                choice: ReviewChoice::Accept,
            })
            .unwrap();
        let mut settings = session.mask.clone();
        settings.start = 1;
        settings.end = 3;
        settings.forward = "ACGTACGTACGT".into();
        settings.evidence = "primer candidate coordinates".into();
        session
            .apply(ReviewOp::Mask {
                settings: settings.clone(),
            })
            .unwrap();
        assert_eq!(session.unresolved(), 0);
        assert_eq!(session.resolved_sequence().as_deref(), Some("CG"));
        session.apply(ReviewOp::Undo).unwrap();
        assert_eq!(session.unresolved(), 2);
        session.apply(ReviewOp::Redo).unwrap();
        assert_eq!(session.mask, settings);
        let review = tmp.path().join("review.json");
        session.save(&review).unwrap();
        std::fs::remove_file(path).unwrap();
        let mut replayed = SangerReviewSession::load(&review).unwrap();
        assert_eq!(replayed.mask, settings);
        assert_eq!(replayed.events(), session.events());
        let output = tmp.path().join("masked.fa");
        replayed.export_consensus(&output, "specimen").unwrap();
        assert_eq!(std::fs::read_to_string(&output).unwrap(), ">specimen\nCG\n");
        // Historical exports are not live review dependencies. Reopening must
        // preserve the manual decisions and undo stack after a folder move or
        // loss/change of an exported FASTA, without rewriting the recorded path.
        replayed.save(&review).unwrap();
        std::fs::write(&output, ">changed\nA\n").unwrap();
        let changed = SangerReviewSession::load(&review).unwrap();
        assert_eq!(changed.events(), replayed.events());
        assert_eq!(changed.resolved_sequence(), replayed.resolved_sequence());
        let export = replayed.events().last().unwrap().clone();
        assert!(replayed.apply(export).is_err(), "live exports verify bytes");
        std::fs::remove_file(&output).unwrap();
        let moved = tmp.path().join("moved");
        std::fs::create_dir(&moved).unwrap();
        let relocated = moved.join("review.json");
        std::fs::rename(&review, &relocated).unwrap();
        let mut recovered = SangerReviewSession::load(&relocated).unwrap();
        assert_eq!(recovered.events(), replayed.events());
        recovered.apply(ReviewOp::Undo).unwrap();
        assert_eq!(recovered.unresolved(), 2);
        recovered.apply(ReviewOp::Redo).unwrap();
        assert_eq!(recovered.resolved_sequence().as_deref(), Some("CG"));
        let mut damaged: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&relocated).unwrap()).unwrap();
        let events = damaged["events"].as_array_mut().unwrap();
        events.last_mut().unwrap()["sha256"] = "not-a-sha256".into();
        std::fs::write(&relocated, serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert!(SangerReviewSession::load(&relocated).is_err());
        let before = replayed.mask.clone();
        let mut invalid = before.clone();
        invalid.start = invalid.end;
        assert!(
            replayed
                .apply(ReviewOp::Mask { settings: invalid })
                .is_err()
        );
        assert_eq!(replayed.mask, before);
    }

    #[test]
    fn thresholds_replay_preserve_manual_decisions_and_masks_and_undo_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("source.json");
        let columns: Vec<_> = "ACGT".chars().enumerate().map(|(i,b)| serde_json::json!({"column": i+1,"consensus":b,"support":[{"read":1,"base":b,"kind":"call"}],"conflict":false,"review":true})).collect();
        let data = serde_json::json!({"status":"draft_requires_review","engine":"Tracy","version_output":"test","engine_sha256":"a".repeat(64),"mode":"basecall","inputs":[{"read":1,"path":"x.ab1","sha256":"b".repeat(64),"orientation":"forward"}],"consensus":"ACGT","consensus_length":4,"minimum_overlap":1,"minimum_identity":0.8,"overlaps":[],"aligned_columns":4,"conflict_columns":0,"columns":columns});
        std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let mut session = SangerReviewSession::new(&path).unwrap();
        let defaults = ReviewSettings::default();
        session
            .apply(ReviewOp::Evaluate {
                version: 2,
                settings: defaults.clone(),
                calls: vec![Some('A'), Some('C'), None, Some('T')],
                reference: None,
            })
            .unwrap();
        assert!(
            session.automatic_only(),
            "initial Evaluate must allow orientation and placement"
        );
        session
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
        assert!(
            !session.automatic_only(),
            "explicit review decisions must protect orientation"
        );
        let mut mask = session.mask.clone();
        mask.start = 1;
        mask.end = 3;
        session
            .apply(ReviewOp::Mask {
                settings: mask.clone(),
            })
            .unwrap();
        let strict = ReviewSettings {
            strong_score: 40,
            reference_percent: 10,
            ..defaults.clone()
        };
        let reference = ReviewReference {
            source_path: "frozen.afa".into(),
            source_sha256: "c".repeat(64),
            counts: vec![Some([1, 2, 3, 4]); 4],
        };
        let evaluate = ReviewOp::Evaluate {
            version: 2,
            settings: strict.clone(),
            calls: vec![None, Some('T'), Some('A'), None],
            reference: Some(reference.clone()),
        };
        session.apply(evaluate.clone()).unwrap();
        assert_eq!(session.decision(1), Some(&ReviewChoice::Edit('G')));
        assert_eq!(session.decision(2), Some(&ReviewChoice::Auto('A')));
        assert_eq!(session.resolved_sequence().as_deref(), Some("GA"));
        assert_eq!(session.mask, mask);
        session.apply(ReviewOp::Undo).unwrap();
        assert_eq!(session.settings, defaults);
        assert_eq!(session.reference, None);
        assert_eq!(session.decision(2), Some(&ReviewChoice::Pending));
        session.apply(ReviewOp::Redo).unwrap();
        assert_eq!(session.settings, strict);
        assert_eq!(session.reference, Some(reference.clone()));
        // Undoing a manual edit restores automatic ownership for later evaluation.
        session
            .apply(ReviewOp::Set {
                columns: vec![2],
                choice: ReviewChoice::Edit('C'),
            })
            .unwrap();
        session.apply(ReviewOp::Undo).unwrap();
        session.apply(evaluate).unwrap();
        assert_eq!(session.decision(2), Some(&ReviewChoice::Auto('A')));
        let before = session.events().len();
        let invalid = ReviewSettings {
            paired_score: 99,
            ..strict.clone()
        };
        assert!(
            session
                .apply(ReviewOp::Evaluate {
                    version: 2,
                    settings: invalid,
                    calls: vec![None; 4],
                    reference: None
                })
                .is_err()
        );
        assert_eq!(session.events().len(), before);
        assert_eq!(session.settings, strict);
        let review = tmp.path().join("review.json");
        session.save(&review).unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut loaded = SangerReviewSession::load(&review).unwrap();
        assert_eq!(loaded.settings, strict);
        assert_eq!(loaded.reference, Some(reference));
        assert_eq!(loaded.mask, mask);
        assert_eq!(loaded.decision(1), Some(&ReviewChoice::Edit('G')));
        let output = tmp.path().join("consensus.fa");
        loaded.export_consensus(&output, "sample").unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), ">sample\nGA\n");
    }

    #[test]
    fn automatic_triage_and_manual_edits_replay_portably() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("draft.json");
        let draft = serde_json::json!({
            "status":"draft_requires_review", "engine":"Tracy", "version_output":"test",
            "engine_sha256":"a".repeat(64), "mode":"basecall",
            "inputs":[{"read":1,"path":"read.ab1","sha256":"b".repeat(64),"orientation":"forward"}],
            "consensus":"AC", "consensus_length":2, "minimum_overlap":1, "minimum_identity":0.8,
            "overlaps":[], "aligned_columns":2, "conflict_columns":0, "limitations":[],
            "columns":[
                {"column":1,"consensus":"A","support":[{"read":1,"base":"A","kind":"call"}],"conflict":false,"review":true},
                {"column":2,"consensus":"C","support":[{"read":1,"base":"C","kind":"call"}],"conflict":false,"review":true}
            ]
        });
        std::fs::write(&source, serde_json::to_vec(&draft).unwrap()).unwrap();
        let mut review = SangerReviewSession::new(&source).unwrap();
        review
            .apply(ReviewOp::AutoAccept { columns: vec![0] })
            .unwrap();
        assert_eq!(review.unresolved(), 1);
        assert!(review.apply(ReviewOp::Undo).is_err());
        review
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('T'),
            })
            .unwrap();
        assert_eq!(review.resolved_sequence().as_deref(), Some("AT"));
        review.apply(ReviewOp::Undo).unwrap();
        assert_eq!(review.unresolved(), 1);
        review.apply(ReviewOp::Redo).unwrap();
        let store = tmp.path().join("review.json");
        review.save(&store).unwrap();
        std::fs::remove_file(&source).unwrap();
        let mut replayed = SangerReviewSession::load(&store).unwrap();
        assert_eq!(replayed.events(), review.events());
        assert_eq!(replayed.resolved_sequence().as_deref(), Some("AT"));
        let output = tmp.path().join("edited.fa");
        replayed.export_consensus(&output, "sample").unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), ">sample\nAT\n");
        replayed
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Pending,
            })
            .unwrap();
        assert!(
            replayed
                .apply(ReviewOp::AutoAccept { columns: vec![0] })
                .is_err()
        );
        assert_eq!(replayed.unresolved(), 1);
    }

    #[test]
    fn sanger_review_ops_replay_batch_undo_and_redo() {
        let path = Path::new("/tmp/basecraft-sanger-review-real-evidence.json");
        if !path.exists() {
            return;
        }
        let mut review = SangerReviewSession::new(path).unwrap();
        let issue = review.draft.columns.iter().position(|c| c.review).unwrap();
        let original = review.decision(issue).unwrap().clone();
        review
            .apply(ReviewOp::Set {
                columns: vec![issue],
                choice: ReviewChoice::Edit('N'),
            })
            .unwrap();
        assert_eq!(review.decision(issue), Some(&ReviewChoice::Edit('N')));
        review.apply(ReviewOp::Undo).unwrap();
        assert_eq!(review.decision(issue), Some(&original));
        review.apply(ReviewOp::Redo).unwrap();
        assert_eq!(review.decision(issue), Some(&ReviewChoice::Edit('N')));
        let store = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        std::fs::remove_file(&store).unwrap();
        review.save(&store).unwrap();
        let replayed = SangerReviewSession::load(&store).unwrap();
        assert_eq!(replayed.events(), review.events());
        assert_eq!(replayed.decision(issue), review.decision(issue));
        assert_eq!(replayed.unresolved(), review.unresolved());
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("draft.json");
        std::fs::copy(path, &source).unwrap();
        let mut complete = SangerReviewSession::new(&source).unwrap();
        let issues = (0..complete.draft.columns.len()).collect();
        complete
            .apply(ReviewOp::Set {
                columns: issues,
                choice: ReviewChoice::Accept,
            })
            .unwrap();
        assert_eq!(complete.unresolved(), 0);
        let fasta = temporary.path().join("specimen.fa");
        complete.export_consensus(&fasta, "specimen").unwrap();
        let portable = tempfile::NamedTempFile::new().unwrap();
        complete.save(portable.path()).unwrap();
        std::fs::remove_file(source).unwrap();
        let loaded = SangerReviewSession::load(portable.path()).unwrap();
        assert_eq!(loaded.resolved_sequence(), complete.resolved_sequence());
        assert!(std::fs::read_to_string(fasta).unwrap().contains('\n'));
    }
}

/// Open instrument evidence immediately, without requiring writable metadata storage.
pub fn instrument_review(path: &Path) -> Result<SangerReviewSession, String> {
    use basecraft_docs::sanger::{
        DraftColumn, InputRead, Orientation, ReadSupport, SupportKind, TraceCalls,
    };
    use std::io::Read;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| e.to_string())?
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("AB1 exceeds 128 MiB".into());
    }
    let trace = basecraft_io::abif::parse(&bytes).map_err(|e| e.to_string())?;
    let calls = trace.display_calls();
    let sequence = String::from_utf8(calls.bases.clone()).map_err(|e| e.to_string())?;
    let length = sequence.len();
    let draft = SangerDraft {
        status: "draft_requires_review".into(), engine: "ABI".into(),
        version_output: format!("ABIF instrument PBAS{}; no recalculation", calls.number),
        engine_sha256: String::new(), mode: "single".into(), signal_threshold: None,
        inputs: vec![InputRead { read: 1, path: path.clone(), sha256: basecraft_io::sha256_bytes(&bytes),
            orientation: Orientation::Forward, calls: Some(TraceCalls { sequence: sequence.clone(), secondary_sequence: None,
                positions: calls.positions.clone(), scores: calls.qualities.as_ref().map_or_else(
                    || vec![0; length], |scores| scores.iter().map(|&score| i32::from(score)).collect()) }) }],
        consensus: sequence.clone(), consensus_length: length, minimum_overlap: 1,
        minimum_identity: 1.0, overlaps: vec![], aligned_columns: length, conflict_columns: 0,
        columns: sequence.chars().enumerate().map(|(i, base)| DraftColumn { column: i + 1,
            consensus: base, support: vec![ReadSupport { read: 1, base, kind: SupportKind::Call }],
            conflict: false, review: true }).collect(),
        limitations: vec!["Instrument calls; missing qualities use zero for conservative triage. Review before use.".into()],
    };
    SangerReviewSession::from_draft(draft, &path)
}

#[cfg(test)]
mod call_set_tests {
    use super::*;

    fn instrument(folder: &Path) -> SangerReviewSession {
        let path = folder.join("trace.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &path,
        )
        .unwrap();
        instrument_review(&path).unwrap()
    }

    fn alternative(session: &SangerReviewSession, version: &str) -> SangerReviewSession {
        let mut draft = session.draft.clone();
        draft.engine = "Tracy".into();
        draft.mode = "basecall".into();
        draft.version_output = version.into();
        draft.engine_sha256 = "a".repeat(64);
        SangerReviewSession::from_draft(draft, &session.draft_path).unwrap()
    }

    #[test]
    fn independent_histories_masks_selection_and_replaced_calls_survive_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = instrument(tmp.path());
        let original = std::fs::read(&session.draft_path).unwrap();
        let instrument_id = session.draft_sha256.clone();
        let mut computed = alternative(&session, "caller 1");
        let computed_id = computed.draft_sha256.clone();
        computed
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('C'),
            })
            .unwrap();
        session
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
        session.retain_call_set(computed).unwrap();
        assert_eq!(session.draft_sha256, instrument_id);
        session.select_call_set(&computed_id).unwrap();
        assert_eq!(session.decision(0), Some(&ReviewChoice::Edit('C')));
        session.apply(ReviewOp::Undo).unwrap();
        session.apply(ReviewOp::Redo).unwrap();
        let mut mask = session.mask.clone();
        mask.start = 1;
        session.apply(ReviewOp::Mask { settings: mask }).unwrap();
        let newer = alternative(&session, "caller 2");
        session.retain_call_set(newer).unwrap();
        let saved = tmp.path().join("review.json");
        session.save(&saved).unwrap();
        let bytes = std::fs::read(&saved).unwrap();
        assert!(
            serde_json::from_slice::<StoredReview>(&bytes).is_err(),
            "legacy reader must refuse v2"
        );
        assert_eq!(evidence_kind(&saved), Some(EvidenceKind::Review));
        let mut reopened = SangerReviewSession::load(&saved).unwrap();
        assert_eq!(reopened.call_sets().len(), 3);
        assert_eq!(reopened.draft_sha256, computed_id);
        assert_eq!(reopened.mask.start, 1);
        reopened.select_call_set(&instrument_id).unwrap();
        assert_eq!(reopened.mask.start, 0);
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
        reopened.select_call_set(&computed_id).unwrap();
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
        assert_eq!(std::fs::read(&session.draft_path).unwrap(), original);
        let package = tmp.path().join("package");
        crate::provenance::package(&saved, &package).unwrap();
        std::fs::remove_file(&session.draft_path).unwrap();
        let packaged = SangerReviewSession::load(&package.join("review.json")).unwrap();
        assert!(packaged.working_draft().is_ok());
        assert!(
            packaged
                .retained
                .iter()
                .all(|set| set.working_draft().is_ok())
        );
    }

    #[test]
    fn all_retained_drafts_and_versions_are_checked_and_v1_still_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = instrument(tmp.path());
        let legacy = tmp.path().join("legacy.json");
        session.save(&legacy).unwrap();
        assert!(SangerReviewSession::load(&legacy).is_ok());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&legacy).unwrap()).unwrap()
                ["version"],
            1
        );
        session
            .retain_call_set(alternative(&session, "caller 1"))
            .unwrap();
        let saved = tmp.path().join("review.json");
        session.save(&saved).unwrap();
        let mut raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
        raw["sets"][1]["draft_sha256"] = "b".repeat(64).into();
        std::fs::write(&saved, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(SangerReviewSession::load(&saved).is_err());
        assert_eq!(evidence_kind(&saved), None);
        raw["version"] = 99.into();
        std::fs::write(&saved, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(SangerReviewSession::load(&saved).is_err());
        let unknown = tmp.path().join("unrelated.sanger-review.json");
        std::fs::write(&unknown, b"{\"other\":true}").unwrap();
        assert_eq!(evidence_kind(&unknown), None);
    }

    #[test]
    fn unavailable_storage_does_not_prevent_instrument_review_or_lose_edits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = instrument(tmp.path());
        std::fs::write(tmp.path().join(".basecraft"), "foreign file").unwrap();
        assert!(session.start_recovery().is_err());
        session
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
        assert_eq!(session.decision(0), Some(&ReviewChoice::Edit('G')));
        assert!(session.save(&tmp.path().join("review.json")).is_err());
        assert_eq!(session.decision(0), Some(&ReviewChoice::Edit('G')));
        assert!(!tmp.path().join("review.json").exists());
    }
}

#[cfg(all(test, unix))]
mod readonly_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    #[test]
    fn readonly_source_folder_keeps_instrument_editing_and_previous_save() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("source.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &path,
        )
        .unwrap();
        let mut review = instrument_review(&path).unwrap();
        let saved = tmp.path().join("review.json");
        review.save(&saved).unwrap();
        let previous = std::fs::read(&saved).unwrap();
        let original = std::fs::read(&path).unwrap();
        let _restore = Restore(tmp.path().into());
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut fresh = instrument_review(&path).unwrap();
        fresh
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('C'),
            })
            .unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
        assert!(review.save(&saved).is_err());
        assert_eq!(std::fs::read(&saved).unwrap(), previous);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(review.decision(0), Some(&ReviewChoice::Edit('G')));
        assert_eq!(fresh.decision(0), Some(&ReviewChoice::Edit('C')));
    }
}

#[cfg(test)]
mod moved_and_failed_recovery_tests {
    use super::*;
    mod recover_cleanup_tests {
        include!("sanger_review/recover_cleanup_tests.rs");
    }
    pub(super) fn pair(folder: &Path) -> SangerReviewSession {
        std::fs::create_dir(folder).unwrap();
        let source = folder.join("source.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &source,
        )
        .unwrap();
        let mut review = instrument_review(&source).unwrap();
        let mut draft = review.draft.clone();
        draft.engine = "Tracy".into();
        draft.mode = "basecall".into();
        draft.version_output = "caller".into();
        let mut alternative = SangerReviewSession::from_draft(draft, &source).unwrap();
        alternative
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('C'),
            })
            .unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('G'),
            })
            .unwrap();
        review.retain_call_set(alternative).unwrap();
        review
    }
    #[test]
    fn stale_review_save_preserves_both_call_sets_and_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("source");
        let mut original = pair(&folder);
        let path = folder.join("review.json");
        original.save(&path).unwrap();
        let mut a = SangerReviewSession::load(&path).unwrap();
        let mut b = SangerReviewSession::load(&path).unwrap();
        a.apply(ReviewOp::Set {
            columns: vec![1],
            choice: ReviewChoice::Edit('A'),
        })
        .unwrap();
        a.save(&path).unwrap();
        let accepted = std::fs::read(&path).unwrap();
        b.start_recovery().unwrap();
        b.apply(ReviewOp::Set {
            columns: vec![1],
            choice: ReviewChoice::Edit('T'),
        })
        .unwrap();
        let other = b.retained[0].draft_sha256.clone();
        b.select_call_set(&other).unwrap();
        b.apply(ReviewOp::Set {
            columns: vec![1],
            choice: ReviewChoice::Edit('G'),
        })
        .unwrap();
        assert!(b.save(&path).is_err(), "stale review must not overwrite A");
        assert_eq!(std::fs::read(&path).unwrap(), accepted);
        assert_eq!(b.decision(1), Some(&ReviewChoice::Edit('G')));
        let mut recovered = SangerReviewSession::load(b.recovery_path().unwrap()).unwrap();
        assert!(recovered.save(&path).is_err());
        assert!(recovered.save(b.recovery_path().unwrap()).is_err());
        assert_eq!(recovered.decision(1), Some(&ReviewChoice::Edit('G')));
        assert_eq!(
            recovered.retained[0].decision(1),
            Some(&ReviewChoice::Edit('T'))
        );
    }

    #[test]
    fn external_missing_recreated_and_save_as_targets_are_guarded() {
        // Registered stores must outlive every iteration in this test profile.
        let mut fixtures = Vec::new();
        for mutation in ["external", "missing", "recreated"] {
            let tmp = tempfile::tempdir().unwrap();
            let folder = tmp.path().join("source");
            let mut review = pair(&folder);
            let path = folder.join("review.json");
            review.save(&path).unwrap();
            let accepted = std::fs::read(&path).unwrap();
            let mut loaded = SangerReviewSession::load(&path).unwrap();
            let other = loaded.retained[0].draft_sha256.clone();
            loaded.select_call_set(&other).unwrap();
            loaded
                .apply(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('T'),
                })
                .unwrap();
            match mutation {
                "external" => std::fs::write(&path, b"external replacement").unwrap(),
                "missing" => std::fs::remove_file(&path).unwrap(),
                _ => {
                    let replacement = folder.join("replacement");
                    std::fs::write(&replacement, &accepted).unwrap();
                    std::fs::rename(&replacement, &path).unwrap();
                }
            }
            let before = std::fs::read(&path).ok();
            assert!(loaded.save(&path).is_err(), "{mutation}");
            assert_eq!(std::fs::read(&path).ok(), before);
            let unknown = folder.join("existing.json");
            std::fs::write(&unknown, b"unrelated").unwrap();
            assert!(
                loaded
                    .save(&unknown)
                    .unwrap_err()
                    .contains("already exists")
            );
            assert_eq!(std::fs::read(&unknown).unwrap(), b"unrelated");
            let new = folder.join("new.json");
            loaded.save(&new).unwrap();
            let reopened = SangerReviewSession::load(&new).unwrap();
            assert_eq!(reopened.decision(1), Some(&ReviewChoice::Edit('T')));
            assert_eq!(
                reopened.retained[0].decision(0),
                Some(&ReviewChoice::Edit('G'))
            );
            fixtures.push(tmp);
        }
    }

    #[cfg(unix)]
    #[test]
    fn aliases_do_not_split_saved_review_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("source");
        let mut review = pair(&folder);
        let path = folder.join("review.json");
        review.save(&path).unwrap();
        let symlink = folder.join("symlink.json");
        std::os::unix::fs::symlink(&path, &symlink).unwrap();
        assert!(review.save(&symlink).unwrap_err().contains("symbolic link"));
        let hardlink = folder.join("hardlink.json");
        std::fs::hard_link(&path, &hardlink).unwrap();
        assert!(review.save(&hardlink).unwrap_err().contains("hard links"));
        assert!(review.save(&path).unwrap_err().contains("hard links"));
        std::fs::remove_file(&hardlink).unwrap();
        let parent_alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&folder, &parent_alias).unwrap();
        review.save(&parent_alias.join("./review.json")).unwrap();
        review
            .save(&folder.join("../source/./review.json"))
            .unwrap();
        let reopened = SangerReviewSession::load(&path).unwrap();
        assert_eq!(reopened.call_sets().len(), 2);
    }

    #[test]
    fn referenced_review_uses_shared_cancel_preserve_and_break_policy() {
        use crate::input_evidence::{self as inputs, SavePolicy};
        // Registered stores must outlive every iteration in this test profile.
        let mut fixtures = Vec::new();
        for policy in [SavePolicy::PreserveCopy, SavePolicy::BreakEvidence] {
            let tmp = tempfile::tempdir().unwrap();
            let folder = tmp.path().join("source");
            let mut review = pair(&folder);
            let path = folder.join("review.json");
            review.save(&path).unwrap();
            let before = std::fs::read(&path).unwrap();
            let ledger = inputs::reference_input_with_dependencies(
                &folder.join("downstream.fa"),
                &path,
                &basecraft_io::hash_file(&path).unwrap(),
                &[],
                "test downstream review",
            )
            .unwrap();
            review
                .apply(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('T'),
                })
                .unwrap();
            assert!(
                review
                    .save(&path)
                    .unwrap_err()
                    .contains("analysis_input_conflict")
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            if policy == SavePolicy::PreserveCopy {
                // Failed preservation must not publish new visible review bytes.
                let preserved = ledger.directory().join("preserved-input");
                std::fs::write(&preserved, b"damaged preservation").unwrap();
                assert!(review.save_with_evidence(&path, policy).is_err());
                assert_eq!(std::fs::read(&path).unwrap(), before);
                std::fs::remove_file(&preserved).unwrap();
            }
            review.save_with_evidence(&path, policy).unwrap();
            let input = inputs::read_input(ledger.directory()).unwrap();
            if policy == SavePolicy::PreserveCopy {
                assert_eq!(input.state, "preserved");
                assert_eq!(
                    std::fs::read(ledger.directory().join("preserved-input")).unwrap(),
                    before
                );
            } else {
                assert_eq!(input.state, "broken");
            }
            assert_eq!(
                SangerReviewSession::load(&path).unwrap().decision(1),
                Some(&ReviewChoice::Edit('T'))
            );
            fixtures.push(tmp);
        }
    }

    #[test]
    fn failed_binding_reports_saved_data_and_allows_safe_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("source");
        let mut review = pair(&folder);
        let path = folder.join("review.json");
        review.save(&path).unwrap();
        review
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('T'),
            })
            .unwrap();
        let key = basecraft_io::hash_bytes(b"review.json");
        let binding = folder
            .join(".basecraft/analyses-v1/bindings")
            .join(key.trim_start_matches("b3:"));
        let previous_binding = std::fs::read(&binding).unwrap();
        std::fs::remove_file(&binding).unwrap();
        std::fs::create_dir(&binding).unwrap();
        let error = review.save(&path).unwrap_err();
        assert!(error.contains("data saved; binding incomplete"), "{error}");
        assert!(error.contains("repair evidence"));
        assert_eq!(
            SangerReviewSession::load(&path).unwrap().decision(1),
            Some(&ReviewChoice::Edit('T'))
        );
        assert_eq!(
            SangerReviewSession::load(review.recovery_path().unwrap())
                .unwrap()
                .call_sets()
                .len(),
            2
        );
        std::fs::remove_dir(&binding).unwrap();
        std::fs::write(&binding, previous_binding).unwrap();
        review.save(&path).unwrap();
        assert!(
            crate::analysis_store::ArtifactRun::for_result(&path)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn simultaneous_review_writers_accept_only_one_revision() {
        fn wait_for(path: &Path) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !path.exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "writer handshake timed out: {}",
                    path.display()
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        // A second OS process uses the actual load/edit/save path, with both
        // writers loaded before either is allowed to publish.
        if let Some(target) = std::env::var_os("BASECRAFT_SANGER_WRITER_TARGET") {
            let path = PathBuf::from(target);
            let mut session = SangerReviewSession::load(&path).unwrap();
            session
                .apply(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('A'),
                })
                .unwrap();
            std::fs::write(path.with_extension("ready"), b"ready").unwrap();
            wait_for(&path.with_extension("go"));
            let result = if session.save(&path).is_ok() {
                "saved"
            } else {
                "refused"
            };
            std::fs::write(path.with_extension("result"), result).unwrap();
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("source");
        let mut review = pair(&folder);
        let path = folder.join("review.json");
        review.save(&path).unwrap();
        let mut parent = SangerReviewSession::load(&path).unwrap();
        parent
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('T'),
            })
            .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sanger_review::moved_and_failed_recovery_tests::simultaneous_review_writers_accept_only_one_revision"])
            .env("BASECRAFT_SANGER_WRITER_TARGET", &path)
            .spawn().unwrap();
        wait_for(&path.with_extension("ready"));
        std::fs::write(path.with_extension("go"), b"go").unwrap();
        let parent_saved = parent.save(&path).is_ok();
        assert!(child.wait().unwrap().success());
        let child_saved =
            std::fs::read_to_string(path.with_extension("result")).unwrap() == "saved";
        assert_ne!(parent_saved, child_saved);
        let loaded = SangerReviewSession::load(&path).unwrap();
        assert_eq!(
            loaded.decision(1),
            Some(&ReviewChoice::Edit(if parent_saved { 'T' } else { 'A' }))
        );
        assert!(
            crate::analysis_store::ArtifactRun::for_result(&path)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn moved_folder_reopens_every_call_set_without_relying_on_cache_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let original = tmp.path().join("original");
        let mut review = pair(&original);
        review.save(&original.join("review.json")).unwrap();
        let moved = tmp.path().join("moved");
        std::fs::rename(original, &moved).unwrap();
        let mut reopened = SangerReviewSession::load(&moved.join("review.json")).unwrap();
        assert!(reopened.working_draft().is_ok());
        let other = reopened.retained[0].draft_sha256.clone();
        reopened.select_call_set(&other).unwrap();
        assert!(reopened.working_draft().is_ok());
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
        reopened.save(&moved.join("saved-again.json")).unwrap();
    }
    #[test]
    fn failed_recovery_and_save_keep_both_histories_and_previous_file() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("source");
        let mut review = pair(&folder);
        let saved = folder.join("review.json");
        review.save(&saved).unwrap();
        let previous = std::fs::read(&saved).unwrap();
        review.recovery = Some(tmp.path().to_path_buf()); // Cannot atomically replace a directory.
        assert!(
            review
                .apply(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('T')
                })
                .is_err()
        );
        assert_eq!(review.decision(0), Some(&ReviewChoice::Edit('G')));
        assert_eq!(review.decision(1), Some(&ReviewChoice::Edit('T')));
        assert_eq!(
            review.retained[0].decision(0),
            Some(&ReviewChoice::Edit('C'))
        );
        let other = review.retained[0].draft_sha256.clone();
        assert!(review.select_call_set(&other).is_err());
        assert_eq!(
            review.draft.engine, "ABI",
            "failed selection checkpoint rolls back selection"
        );
        assert!(review.save(&saved).is_err());
        assert_eq!(std::fs::read(&saved).unwrap(), previous);
        review.recovery = None;
        review.save(&saved).unwrap();
        let reopened = SangerReviewSession::load(&saved).unwrap();
        assert_eq!(reopened.decision(1), Some(&ReviewChoice::Edit('T')));
        assert_eq!(
            reopened.retained[0].decision(0),
            Some(&ReviewChoice::Edit('C'))
        );
    }
}
