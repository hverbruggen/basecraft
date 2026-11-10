//! Transient save decisions. No consent is written to disk or deserialised.
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{DocId, Session};
use crate::input_evidence::{EvidenceCheck, InspectionIssue, SavePolicy};
use crate::{Result, error::invalid};

/// A fresh advisory check, or the fresh scope returned after refusing stale
/// consent. Fields cannot be changed to manufacture authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SaveCheck {
    evidence: EvidenceCheck,
    session: String,
    document: usize,
    document_key: String,
    generation: u64,
    revision: u64,
    accepted_hashes: Vec<(String, String)>,
    target: PathBuf,
    target_revision: Option<TargetRevision>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct TargetRevision {
    hash: String,
    stamp: basecraft_protocol::provenance::FileStamp,
}

impl SaveCheck {
    pub fn evidence(&self) -> &EvidenceCheck {
        &self.evidence
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Consumes this snapshot. `acknowledge_incomplete` is independent of the
    /// policy for known analyses. Neither snapshot nor decision is Clone or
    /// Deserialize; a failed attempt requires a fresh check and decision.
    pub fn decide(self, known: SavePolicy, acknowledge_incomplete: bool) -> SaveDecision {
        SaveDecision {
            check: self,
            known,
            acknowledge_incomplete,
        }
    }
}

#[derive(Debug)]
pub struct SaveDecision {
    pub(super) check: SaveCheck,
    pub(super) known: SavePolicy,
    pub(super) acknowledge_incomplete: bool,
}

#[derive(Debug, Serialize)]
pub struct SaveOutcome {
    pub warnings: Vec<SaveWarning>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SaveWarning {
    EvidenceNotChecked { issues: Vec<InspectionIssue> },
}

impl Session {
    /// Advisory and side-effect-free. No lock is retained across caller UI.
    /// The authoritative save always repeats this inspection under its guard.
    pub fn check_save(&self, id: DocId, path: Option<&Path>) -> Result<SaveCheck> {
        if !self.is_open(id) {
            return Err(invalid("save document is not open"));
        }
        let target = path.unwrap_or_else(|| Path::new(&self.docs[id.0].key));
        let target = if path.is_none() {
            self.resolve(&self.docs[id.0].key)
        } else {
            target.into()
        };
        let evidence = crate::input_evidence::check(&target)?;
        self.save_check_scope(id, &target, evidence)
    }

    pub(super) fn save_check_scope(
        &self,
        id: DocId,
        target: &Path,
        evidence: EvidenceCheck,
    ) -> Result<SaveCheck> {
        let target = crate::save_guard::target_path(target)?;
        // Unreadable/unsafe destinations are actual target errors, never an
        // incomplete-analysis issue that acknowledgement can waive.
        let target_revision = match std::fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(invalid("save target must be an ordinary file"));
                }
                Some(TargetRevision {
                    hash: basecraft_io::hash_file(&target)?,
                    stamp: crate::input_evidence::stamp(&target)?,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(SaveCheck {
            evidence,
            session: self.id.clone(),
            document: id.0,
            document_key: self.docs[id.0].key.clone(),
            generation: self.generation(id),
            revision: self.revision(id),
            accepted_hashes: self.docs[id.0].accepted_hashes.clone(),
            target,
            target_revision,
        })
    }

    /// Explicit one-attempt decision, including overwrite confirmation supplied
    /// by the caller. Adapters without overwrite confirmation use save_new_with_decision.
    pub fn save_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(id, path, decision.known, true, false, Some(decision))
    }

    /// The same decision boundary while retaining save_new's no-overwrite rule.
    pub fn save_new_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(id, path, decision.known, false, false, Some(decision))
    }
}
