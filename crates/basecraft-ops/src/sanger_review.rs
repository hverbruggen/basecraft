//! Replayable review decisions for a bounded Sanger consensus draft.
//! The source draft and AB1 files remain immutable; no reference base is
//! inferred or substituted by these operations.

mod captured;
mod edit;
pub use captured::CapturedInput;
mod storage;
use basecraft_docs::sanger::SangerDraftExt;
pub use storage::{CheckedAddition, SangerRun, check_addition, record_addition};
pub(crate) use storage::{is_review, relocate_package};

use std::path::{Path, PathBuf};

use basecraft_docs::sanger::{ReviewReference, ReviewSettings, SangerDraft};

use crate::error::damaged;
use crate::{Error, Result};

/// The largest Sanger draft or placement basecraft reads.
const EVIDENCE_LIMIT: u64 = 32 * 1024 * 1024;
/// The largest saved Sanger review basecraft reads.
const REVIEW_LIMIT: u64 = 64 * 1024 * 1024;

/// Read Sanger evidence JSON (a draft or a placement) and check it.
pub fn read_evidence<T: serde::de::DeserializeOwned>(
    path: &Path,
    check: impl Fn(&T) -> std::result::Result<(), String>,
) -> Result<T> {
    parse_evidence(path, &std::fs::read(path)?, check)
}

fn parse_evidence<T: serde::de::DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
    check: impl Fn(&T) -> std::result::Result<(), String>,
) -> Result<T> {
    if bytes.len() as u64 > EVIDENCE_LIMIT {
        return Err(Error::Limit {
            what: "Sanger evidence bytes".into(),
            limit: EVIDENCE_LIMIT,
        });
    }
    let value: T = serde_json::from_slice(bytes).map_err(|e| damaged(path, e.to_string()))?;
    check(&value).map_err(|e| damaged(path, e))?;
    Ok(value)
}

/// Recognised saved Sanger evidence, determined from validated content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceKind {
    Draft,
    Review,
}

/// Inspect JSON without creating records or treating arbitrary JSON as a trace.
pub fn evidence_kind(path: &Path) -> Option<EvidenceKind> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if std::io::BufReader::new(file)
        .bytes()
        .find_map(|byte| match byte {
            Ok(byte) if byte.is_ascii_whitespace() => None,
            other => Some(other.ok()),
        })??
        != b'{'
    {
        return None;
    }
    let bytes = crate::records::provenance::read_record(path).ok()?;
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && value.get("version").is_some()
    {
        return SangerReview::decode_value(value, path)
            .ok()
            .map(|_| EvidenceKind::Review);
    }
    let draft: SangerDraft = serde_json::from_slice(&bytes).ok()?;
    draft.validate().ok()?;
    Some(EvidenceKind::Draft)
}

/// Read a validated saved review recognised by content. Other files return None.
pub fn saved_review(path: &Path) -> Option<Result<SangerReview>> {
    (evidence_kind(path) == Some(EvidenceKind::Review)).then(|| SangerReview::load(path))
}

pub use basecraft_protocol::sanger::{MaskSettings, ReviewChoice, ReviewOp};

use basecraft_docs::sanger::review::ReviewChange as Change;

use basecraft_protocol::sanger_records::{
    SavedCallSets as StoredCallSets, SavedReview as StoredReview,
};
use storage::EvidencePaths;

#[derive(Clone)]
pub struct SangerReview {
    // Only the selected session owns the inactive sets. Each set owns its replay history.
    retained: Vec<SangerReview>,
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
    evidence: storage::ReviewEvidence,
    anchor: PathBuf,
    captures: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<PathBuf, CapturedInput>>>,
}

impl SangerReview {
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
    /// Combine baseline scientific decisions with chronological shared history once.
    pub fn report_with_history(
        &self,
        record: &crate::records::document_record::DocumentRecord,
    ) -> Result<crate::records::document_record::Report> {
        let mut report = self.document_report(record.unsaved)?;
        let mut represented = std::collections::HashSet::new();
        let mut duplicate_rows = std::collections::HashSet::new();
        for set in std::iter::once(self).chain(self.retained.iter()) {
            for (index, edit) in set.events.iter().enumerate() {
                if let Some(entry) = record.events.iter().find(|entry| !represented.contains(&entry.id) && matches!(&entry.op,basecraft_protocol::Op::SangerReview {call_set,edit:logged} if call_set == &set.draft_sha256 && logged.as_ref()==edit)) {
                    represented.insert(entry.id.clone());
                    duplicate_rows.insert(format!("review:{}:{index}",set.draft_sha256));
                }
            }
        }
        report
            .history
            .retain(|row| !duplicate_rows.contains(&row.id));
        let open_ids: std::collections::HashSet<_> = record
            .events
            .iter()
            .filter(|entry| matches!(entry.op, basecraft_protocol::Op::Open { .. }))
            .map(|entry| entry.id.as_str())
            .collect();
        let shared = record.report()?;
        report.warnings.extend(shared.warnings);
        report.history.extend(
            shared
                .history
                .into_iter()
                .filter(|row| !open_ids.contains(row.id.as_str())),
        );
        report.history_text = report
            .history
            .iter()
            .map(|row| row.summary.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(report)
    }
    /// Describe the actual review state, retaining the complete decision audit separately.
    pub fn document_report(
        &self,
        unsaved: bool,
    ) -> Result<crate::records::document_record::Report> {
        use crate::records::document_record::{DocumentRecord, HistoryRow, MethodsParagraph};
        let mut state = serde_json::json!({
            "draft": self.draft, "draft_sha256": self.draft_sha256, "events": self.events,
            "settings": self.settings, "reference": self.reference, "mask": self.mask,
            "decisions": self.decisions, "manual": self.manual,
        });
        // Every retained call set is part of the revision.
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
        let raw = serde_json::to_string_pretty(&state)?;
        let mut record = DocumentRecord::empty(&self.draft_path.display().to_string());
        record.revision = basecraft_io::hash_bytes(raw.as_bytes());
        record.unsaved = unsaved;
        let mut report = record.report()?;
        report.raw = raw;
        for (index, event) in self.events.iter().enumerate() {
            let summary = describe(event);
            report.history.push(HistoryRow {
                id: format!("review:{}:{index}", self.draft_sha256),
                time: String::new(),
                summary,
                status: "Review audit".into(),
                contributing: false,
                details: serde_json::to_string_pretty(event)?,
            });
        }
        for (set, details) in self.retained.iter().zip(&retained) {
            report.history.extend(set.document_report(unsaved)?.history);

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
                details: serde_json::to_string_pretty(details)?,
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

    pub fn new(draft_path: &Path) -> Result<Self> {
        let draft_json = std::fs::read_to_string(draft_path)?;
        let draft: SangerDraft =
            parse_evidence(draft_path, draft_json.as_bytes(), SangerDraft::validate)?;
        let mut session = Self::from_draft(draft, draft_path)?;
        session.draft_sha256 = basecraft_io::sha256_bytes(draft_json.as_bytes());
        session.draft_json = draft_json;
        Ok(session)
    }

    /// Start an unsaved review without creating files or invoking a caller.
    pub fn from_draft(draft: SangerDraft, draft_path: &Path) -> Result<Self> {
        draft.validate().map_err(Error::Invalid)?;
        let draft_json = serde_json::to_string(&draft)?;
        let hash = basecraft_io::sha256_bytes(draft_json.as_bytes());
        let decisions = vec![ReviewChoice::Pending; draft.columns.len()];
        Ok(Self {
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
            evidence: storage::ReviewEvidence::default(),
            anchor: std::path::absolute(draft_path)?,
            captures: std::sync::Arc::default(),
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        if bytes.len() as u64 > REVIEW_LIMIT {
            return Err(Error::Limit {
                what: "Sanger review bytes".into(),
                limit: REVIEW_LIMIT,
            });
        }
        Self::decode(&bytes, path)
    }

    pub(crate) fn decode(bytes: &[u8], path: &Path) -> Result<Self> {
        let (value, writer) =
            basecraft_protocol::records::decode_written::<serde_json::Value>(bytes)
                .map_err(|e| damaged(path, e.to_string()))?;
        Self::decode_value(value, path).map_err(|e| match e {
            Error::Damaged { path, detail } => damaged(
                path,
                basecraft_protocol::records::unreadable(&writer, &detail).to_string(),
            ),
            other => other,
        })
    }

    fn decode_value(value: serde_json::Value, path: &Path) -> Result<Self> {
        let damaged = |detail: &str| damaged(path, detail);
        let stored: StoredCallSets =
            serde_json::from_value(value).map_err(|e| damaged(&e.to_string()))?;
        if stored.version != 3 {
            return Err(Error::Unsupported(
                "Unsupported Sanger review version".into(),
            ));
        }
        if stored.sets.is_empty() || stored.sets.len() > 32 {
            return Err(damaged("Invalid number of Sanger call sets"));
        }
        let mut sets = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for set in stored.sets {
            let review = Self::from_stored(set, path)?;
            if !ids.insert(review.draft_sha256.clone()) {
                return Err(damaged("Duplicate Sanger call set"));
            }
            sets.push(review);
        }
        let selected = sets
            .iter()
            .position(|set| set.draft_sha256 == stored.selected)
            .ok_or_else(|| damaged("Selected Sanger call set is missing"))?;
        let mut review = sets.remove(selected);
        if !sets.is_empty()
            && (review.draft.inputs.len() != 1
                || sets.iter().any(|set| {
                    set.draft.inputs.len() != 1
                        || set.draft.inputs[0].sha256 != review.draft.inputs[0].sha256
                }))
        {
            return Err(damaged(
                "Retained call sets refer to different Sanger sources",
            ));
        }
        review.retained = sets;
        Ok(review)
    }

    fn from_stored(stored: StoredReview, path: &Path) -> Result<Self> {
        let damaged = |detail: &str| damaged(path, detail);
        if basecraft_io::sha256_bytes(stored.draft_json.as_bytes()) != stored.draft_sha256 {
            return Err(damaged("Embedded source draft changed since review"));
        }
        let draft: SangerDraft =
            serde_json::from_str(&stored.draft_json).map_err(|e| damaged(&e.to_string()))?;
        draft.validate().map_err(|e| damaged(&e))?;
        let decisions = vec![ReviewChoice::Pending; draft.columns.len()];
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let mut evidence = stored.evidence;
        if evidence.schema_version != 1 {
            return Err(Error::Unsupported(
                "Unsupported Sanger support version".into(),
            ));
        }
        evidence.map_paths(|p| Ok(parent.join(p)))?;
        let mut session = Self {
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
            evidence,
            anchor: std::path::absolute(path)?,
            captures: std::sync::Arc::default(),
        };
        for event in stored.events {
            if let ReviewOp::Export { sha256, .. } = &event {
                // An export is historical evidence, not an input to review
                // decisions. Moving/removing that FASTA must not make the
                // embedded review unrecoverable. Live apply still checks bytes.
                if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(damaged("Invalid exported consensus SHA-256"));
                }
            } else {
                let change = session.prepare_change(&event).map_err(|e| damaged(&e))?;
                session
                    .install_change(&change, true)
                    .map_err(|e| damaged(&e))?;
            }
            session.events.push(event);
        }
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
    pub fn retain_call_set(&mut self, session: Self) -> Result<()> {
        if !session.retained.is_empty() {
            return Err(Error::Invalid("Sanger call sets cannot be nested".into()));
        }
        if self.retained.len() >= 31 {
            return Err(Error::Limit {
                what: "Sanger call sets".into(),
                limit: 32,
            });
        }
        if self.draft.inputs.len() != 1
            || session.draft.inputs.len() != 1
            || !self.draft.inputs[0]
                .sha256
                .eq_ignore_ascii_case(&session.draft.inputs[0].sha256)
        {
            return Err(Error::Invalid(
                "Sanger call sets refer to different source bytes".into(),
            ));
        }
        if !self
            .call_sets()
            .iter()
            .any(|(id, _)| *id == session.draft_sha256)
        {
            self.retained.push(session);
        }
        Ok(())
    }

    pub fn select_call_set(&mut self, id: &str) -> Result<()> {
        if self.draft_sha256 == id {
            return Ok(());
        }
        let index = self
            .retained
            .iter()
            .position(|set| set.draft_sha256 == id)
            .ok_or_else(|| Error::Invalid("Unknown Sanger call set".into()))?;
        let mut retained = std::mem::take(&mut self.retained);
        std::mem::swap(self, &mut retained[index]);
        // Selection swaps the same two positions; inverse selection restores exact ordering.
        std::mem::swap(&mut self.anchor, &mut retained[index].anchor);
        self.retained = retained;
        Ok(())
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

    /// Export only a fully resolved specimen sequence. The output hash is a
    /// replayable operation; source AB1 and draft paths can never be replaced.
    pub(crate) fn write_consensus(&self, path: &Path, label: &str) -> Result<String> {
        let sequence = self
            .resolved_sequence()
            .ok_or_else(|| Error::NotReady("Resolve every review column before export".into()))?;
        if sequence.is_empty() {
            return Err(Error::NotReady("Resolved consensus is empty".into()));
        }
        if path.exists()
            || path == self.draft_path
            || self.draft.inputs.iter().any(|read| read.path == path)
        {
            return Err(Error::OutputConflict(
                "Choose a new output file distinct from the source traces".into(),
            ));
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
            return Err(Error::MissingChoice("Give the specimen a name".into()));
        }
        let fasta = format!(">{label}\n{sequence}\n");
        // Same-filesystem no-clobber promotion protects a file created after
        // preflight; do not replace another writer's output.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        std::io::Write::write_all(staged.as_file_mut(), fasta.as_bytes())?;
        staged.as_file().sync_all()?;
        staged.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(basecraft_io::sha256_file(path)?)
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

pub(crate) fn read_journal_record(
    record: &basecraft_protocol::sanger_records::ReviewRecord,
) -> Result<StoredCallSets> {
    if record.hash.len() != 67
        || !record.hash.starts_with("b3:")
        || !record.hash[3..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(damaged(record.path.clone(), "Invalid review record digest"));
    }
    let bytes = crate::records::provenance::read_record(&record.path)?;
    if basecraft_io::hash_bytes(&bytes) != record.hash {
        return Err(damaged(
            record.path.clone(),
            "Sanger analysis record changed",
        ));
    }
    Ok(basecraft_protocol::records::decode(&bytes)?)
}

pub fn instrument_review(path: &Path) -> Result<SangerReview> {
    use basecraft_docs::sanger::{
        DraftColumn, InputRead, Orientation, ReadSupport, SupportKind, TraceCalls,
    };
    let capture = CapturedInput::open(path)?;
    let path = capture.path().to_path_buf();
    let trace = basecraft_io::abif::parse(capture.bytes())?;
    let calls = trace.display_calls();
    let sequence =
        String::from_utf8(calls.bases.clone()).map_err(|e| damaged(&path, e.to_string()))?;
    let length = sequence.len();
    let draft = SangerDraft { argv: Vec::new(),
        status: "draft_requires_review".into(), engine: "ABI".into(),
        version_output: format!("ABIF instrument PBAS{}; no recalculation", calls.number),
        engine_sha256: String::new(), mode: "single".into(), signal_threshold: None,
        inputs: vec![InputRead { read: 1, path: path.clone(), sha256: capture.sha256().into(),
            orientation: Orientation::Forward, calls: Some(TraceCalls { signal_bounds: None, sequence: sequence.clone(), secondary_sequence: None,
                positions: calls.positions.clone(), scores: calls.qualities.as_ref().map_or_else(
                    || vec![0; length], |scores| scores.iter().map(|&score| i32::from(score)).collect()) }) }],
        consensus: sequence.clone(), consensus_length: length, minimum_overlap: 1,
        minimum_identity: 1.0, overlaps: vec![], aligned_columns: length, conflict_columns: 0,
        columns: sequence.chars().enumerate().map(|(i, base)| DraftColumn { column: i + 1,
            consensus: base, support: vec![ReadSupport { read: 1, base, kind: SupportKind::Call }],
            conflict: false, review: true }).collect(),
        limitations: vec!["Instrument calls; missing qualities use zero for conservative triage. Review before use.".into()],
    };
    let review = SangerReview::from_draft(draft, &path)?;
    review
        .captures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(path, capture);
    Ok(review)
}

#[cfg(test)]
mod test_session;
#[cfg(test)]
use test_session::ReviewSession;

#[cfg(test)]
mod call_set_tests {
    use super::*;

    fn instrument(folder: &Path) -> ReviewSession {
        let path = folder.join("trace.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &path,
        )
        .unwrap();
        ReviewSession::new(&instrument_review(&path).unwrap())
    }

    fn alternative(session: &SangerReview, version: &str) -> ReviewSession {
        let mut draft = session.draft.clone();
        draft.engine = "Tracy".into();
        draft.mode = "basecall".into();
        draft.version_output = version.into();
        draft.engine_sha256 = "a".repeat(64);
        ReviewSession::new(&SangerReview::from_draft(draft, &session.draft_path).unwrap())
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
        session.retain_call_set(&computed).unwrap();
        assert_eq!(session.draft_sha256, instrument_id);
        session.select_call_set(&computed_id).unwrap();
        assert_eq!(session.decision(0), Some(&ReviewChoice::Edit('C')));
        let mut mask = session.mask.clone();
        mask.start = 1;
        session.apply(ReviewOp::Mask { settings: mask }).unwrap();
        let newer = alternative(&session, "caller 2");
        session.retain_call_set(&newer).unwrap();
        let saved = tmp.path().join("review.json");
        session.save(&saved).unwrap();
        let bytes = std::fs::read(&saved).unwrap();
        assert!(
            serde_json::from_slice::<StoredReview>(&bytes).is_err(),
            "an individual call set is not a saved envelope"
        );
        assert_eq!(evidence_kind(&saved), Some(EvidenceKind::Review));
        let mut reopened = ReviewSession::load(&saved).unwrap();
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
        crate::records::provenance::package(&saved, &package, None).unwrap();
        std::fs::remove_file(&session.draft_path).unwrap();
        let packaged = ReviewSession::load(&package.join("review.json")).unwrap();
        assert!(packaged.working_draft().is_ok());
        assert!(
            packaged
                .retained
                .iter()
                .all(|set| set.working_draft().is_ok())
        );
    }

    #[test]
    fn all_retained_drafts_and_current_version_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = instrument(tmp.path());
        let single = tmp.path().join("single.json");
        session.save(&single).unwrap();
        assert!(ReviewSession::load(&single).is_ok());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&single).unwrap()).unwrap()
                ["version"],
            3
        );
        session
            .retain_call_set(&alternative(&session, "caller 1"))
            .unwrap();
        let saved = tmp.path().join("review.json");
        session.save(&saved).unwrap();
        let mut raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
        raw["sets"][1]["draft_sha256"] = "b".repeat(64).into();
        std::fs::write(&saved, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(ReviewSession::load(&saved).is_err());
        assert_eq!(evidence_kind(&saved), None);
        raw["version"] = 99.into();
        std::fs::write(&saved, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(ReviewSession::load(&saved).is_err());
        let unknown = tmp.path().join("unrelated.sanger-review.json");
        std::fs::write(&unknown, b"{\"other\":true}").unwrap();
        assert_eq!(evidence_kind(&unknown), None);
    }

    #[test]
    fn a_folder_that_cannot_hold_recovery_refuses_the_review_and_says_where() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".basecraft"), "foreign file").unwrap();
        let path = tmp.path().join("trace.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &path,
        )
        .unwrap();
        let review = instrument_review(&path).unwrap();
        let mut owner = crate::Session::recoverable(tmp.path()).unwrap();
        match owner.create_sanger_review(&review) {
            Err(error @ crate::Error::ReadOnlyFolder { .. }) => {
                assert_eq!(error.code(), "read_only_folder");
                assert!(
                    error
                        .to_string()
                        .contains(&tmp.path().display().to_string())
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            std::fs::read(tmp.path().join(".basecraft")).unwrap(),
            b"foreign file"
        );
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
        let mut review = ReviewSession::new(&instrument_review(&path).unwrap());
        let saved = tmp.path().join("review.json");
        review.save(&saved).unwrap();
        let previous = std::fs::read(&saved).unwrap();
        let original = std::fs::read(&path).unwrap();
        let _restore = Restore(tmp.path().into());
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut fresh = ReviewSession::new(&instrument_review(&path).unwrap());
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
    pub(super) fn pair(folder: &Path) -> ReviewSession {
        std::fs::create_dir(folder).unwrap();
        let source = folder.join("source.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &source,
        )
        .unwrap();
        let mut review = ReviewSession::new(&instrument_review(&source).unwrap());
        let mut draft = review.draft.clone();
        draft.engine = "Tracy".into();
        draft.mode = "basecall".into();
        draft.version_output = "caller".into();
        let mut alternative =
            ReviewSession::new(&SangerReview::from_draft(draft, &source).unwrap());
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
        review.retain_call_set(&alternative).unwrap();
        review
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
            let mut loaded = ReviewSession::load(&path).unwrap();
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
                    .to_string()
                    .contains("already exists")
            );
            assert_eq!(std::fs::read(&unknown).unwrap(), b"unrelated");
            let new = folder.join("new.json");
            loaded.save(&new).unwrap();
            let reopened = ReviewSession::load(&new).unwrap();
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
        assert!(
            review
                .save(&symlink)
                .unwrap_err()
                .to_string()
                .contains("symbolic link")
        );
        let hardlink = folder.join("hardlink.json");
        std::fs::hard_link(&path, &hardlink).unwrap();
        assert!(
            review
                .save(&hardlink)
                .unwrap_err()
                .to_string()
                .contains("hard links")
        );
        assert!(
            review
                .save(&path)
                .unwrap_err()
                .to_string()
                .contains("hard links")
        );
        std::fs::remove_file(&hardlink).unwrap();
        let parent_alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&folder, &parent_alias).unwrap();
        review.save(&parent_alias.join("./review.json")).unwrap();
        review
            .save(&folder.join("../source/./review.json"))
            .unwrap();
        let reopened = ReviewSession::load(&path).unwrap();
        assert_eq!(reopened.call_sets().len(), 2);
    }

    #[test]
    fn referenced_review_uses_shared_cancel_preserve_and_break_policy() {
        use crate::records::input_evidence::{self as inputs, SavePolicy};
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
                None,
            )
            .unwrap();
            review
                .apply(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('T'),
                })
                .unwrap();
            assert!(review.save(&path).unwrap_err().code() == "analysis_input_conflict");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            if policy == SavePolicy::PreserveCopy {
                // Failed preservation must not publish new visible review bytes.
                let preserved = ledger.directory().join("preserved-input");
                std::fs::write(&preserved, b"damaged preservation").unwrap();
                assert!(review.save_with_policy(&path, policy).is_err());
                assert_eq!(std::fs::read(&path).unwrap(), before);
                std::fs::remove_file(&preserved).unwrap();
            }
            review.save_with_policy(&path, policy).unwrap();
            let input = inputs::read_input(ledger.directory()).unwrap();
            if policy == SavePolicy::PreserveCopy {
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::EvidenceState::Preserved
                );
                assert_eq!(
                    std::fs::read(ledger.directory().join("preserved-input")).unwrap(),
                    before
                );
            } else {
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::EvidenceState::Broken
                );
            }
            assert_eq!(
                ReviewSession::load(&path).unwrap().decision(1),
                Some(&ReviewChoice::Edit('T'))
            );
            fixtures.push(tmp);
        }
    }

    #[test]
    fn failed_binding_preserves_data_and_allows_safe_retry() {
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
        let before = std::fs::read(&path).unwrap();
        let error = review.save(&path).unwrap_err();
        assert!(error.to_string().contains("binding"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(review.decision(1), Some(&ReviewChoice::Edit('T')));
        std::fs::remove_dir(&binding).unwrap();
        std::fs::write(&binding, previous_binding).unwrap();
        review.save(&path).unwrap();
        assert!(
            crate::records::analysis_store::ArtifactRun::for_result(&path)
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
            let mut session = ReviewSession::load(&path).unwrap();
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
        let mut parent = ReviewSession::load(&path).unwrap();
        parent
            .apply(ReviewOp::Set {
                columns: vec![1],
                choice: ReviewChoice::Edit('T'),
            })
            .unwrap();
        let test = crate::isolated_profile::test_name();
        let child = crate::isolated_profile::relaunch(&test)
            .env("BASECRAFT_SANGER_WRITER_TARGET", &path)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        wait_for(&path.with_extension("ready"));
        std::fs::write(path.with_extension("go"), b"go").unwrap();
        let parent_saved = parent.save(&path).is_ok();
        crate::isolated_profile::assert_one_passed(&test, &child.wait_with_output().unwrap());
        let child_saved =
            std::fs::read_to_string(path.with_extension("result")).unwrap() == "saved";
        assert_ne!(parent_saved, child_saved);
        let loaded = ReviewSession::load(&path).unwrap();
        assert_eq!(
            loaded.decision(1),
            Some(&ReviewChoice::Edit(if parent_saved { 'T' } else { 'A' }))
        );
        assert!(
            crate::records::analysis_store::ArtifactRun::for_result(&path)
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
        let mut reopened = ReviewSession::load(&moved.join("review.json")).unwrap();
        assert!(reopened.working_draft().is_ok());
        let other = reopened.retained[0].draft_sha256.clone();
        reopened.select_call_set(&other).unwrap();
        assert!(reopened.working_draft().is_ok());
        assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
        reopened.save(&moved.join("saved-again.json")).unwrap();
    }
}

pub(crate) fn describe(event: &ReviewOp) -> String {
    match event {
        ReviewOp::AutoAccept { columns } => {
            format!("Automatically accepted {} review columns", columns.len())
        }
        ReviewOp::Evaluate { .. } => {
            "Evaluated automatic calls with recorded review settings".into()
        }
        ReviewOp::Set { columns, choice } => {
            format!(
                "Set {} review columns to {}",
                columns.len(),
                match choice {
                    ReviewChoice::Accept => "accepted".into(),
                    ReviewChoice::Reject => "rejected".into(),
                    ReviewChoice::Pending => "pending".into(),
                    ReviewChoice::Edit(base) | ReviewChoice::Auto(base) => base.to_string(),
                }
            )
        }
        ReviewOp::Mask { settings } => format!(
            "Retained review columns {}–{}",
            settings.start.saturating_add(1),
            settings.end
        ),
        ReviewOp::Export { .. } => "Exported reviewed consensus".into(),
    }
}
