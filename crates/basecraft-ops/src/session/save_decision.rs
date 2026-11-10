//! Transient save decisions. No consent is written to disk or deserialised.
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{DocId, Session};
use crate::input_evidence::{EvidenceCheck, InspectionIssue, SavePolicy};
use crate::{Result, error::invalid};

/// A fresh advisory check, or the fresh scope returned after refusing stale
/// consent. Fields cannot be changed to manufacture authority.
#[derive(Clone, Debug, Serialize)]
pub struct SaveCheck {
    token: String,
    #[serde(flatten)]
    scope: SaveScope,
    #[serde(skip)]
    scope_revision: String,
}

#[derive(Clone, Debug, Serialize)]
struct SaveScope {
    evidence: EvidenceCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    additional_evidence: Vec<EvidenceCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publication: Option<PathBuf>,
    finalization: bool,
    session: String,
    document: usize,
    document_key: String,
    generation: u64,
    revision: u64,
    accepted_hashes: Vec<(String, String)>,
    target: PathBuf,
    target_revision: Option<TargetRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TargetRevision {
    hash: String,
    stamp: basecraft_protocol::provenance::FileStamp,
}

impl SaveCheck {
    pub fn evidence(&self) -> &EvidenceCheck {
        &self.scope.evidence
    }

    pub fn document(&self) -> usize {
        self.scope.document
    }

    pub fn target(&self) -> &Path {
        &self.scope.target
    }

    /// Session-local, single-attempt token for a later adapter request.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Ordered, target-specific checks. Policy capabilities and backing ledgers
    /// remain separate even when two groups name the same logical analysis.
    pub fn evidence_groups(&self) -> impl Iterator<Item = &EvidenceCheck> {
        std::iter::once(&self.scope.evidence).chain(&self.scope.additional_evidence)
    }

    pub(super) fn complete(&self) -> bool {
        self.evidence_groups().all(EvidenceCheck::is_complete)
    }

    pub(super) fn issues(&self) -> Vec<InspectionIssue> {
        self.evidence_groups()
            .flat_map(|group| group.issues.clone())
            .collect()
    }

    pub(super) fn resolve(&self, policy: SavePolicy) -> Result<()> {
        // Validate EVERY group before the first durable evidence mutation.
        for group in self.evidence_groups() {
            if group.conflict.analyses.is_empty() {
                continue;
            }
            if policy == SavePolicy::Cancel {
                return Err(crate::Error::EvidenceConflict(Box::new(
                    group.conflict.clone(),
                )));
            }
            if !group.conflict.options.contains(&policy) {
                return Err(invalid(
                    "old analysed bytes are unavailable; preserve-copy cannot repair this input",
                ));
            }
        }
        for group in self.evidence_groups() {
            crate::input_evidence::resolve(&group.conflict, policy)?;
        }
        Ok(())
    }

    pub(super) fn same_scope(&self, other: &Self) -> bool {
        self.scope_revision == other.scope_revision
    }

    pub(super) fn target_is_current(&self, target: &Path) -> Result<bool> {
        Ok(crate::save_guard::target_path(target)? == self.scope.target
            && target_revision(target)? == self.scope.target_revision)
    }

    /// Consumes this advisory snapshot. `acknowledge_incomplete` is independent
    /// of known policy. `SaveDecision` is neither `Clone` nor `Deserialize`; Session
    /// also consumes the token, so cloning an advisory snapshot cannot replay it.
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
    #[serde(skip)]
    pub(crate) recovery_observation: Option<SaveCheck>,
    pub warnings: Vec<SaveWarning>,
}

#[derive(Clone, Debug, Serialize)]
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
        if self.docs[id.0].accepted_hashes.is_empty() && path.is_none() {
            return Err(invalid(
                "Choose a destination with Save as for this new sequence",
            ));
        }
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        let evidence = crate::input_evidence::check(&target)?;
        let check = self.save_check_scope(id, &target, evidence)?;
        self.publish_save_check(id, check)
    }

    /// Bind a wire acknowledgement to the Session's one pending check for this
    /// document. This does not rescan. `save_with_decision`/`save_new_with_decision`
    /// consume its authority on entry and recheck under the normal guard.
    pub fn decide_save(
        &self,
        id: DocId,
        path: Option<&Path>,
        token: &str,
        known: SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<SaveDecision> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("save document is not open"))?;
        let pending = slot
            .pending_save_check
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))?;
        if let Some(check) = pending.as_ref().filter(|check| check.token == token) {
            return Ok(check.clone().decide(known, acknowledge_incomplete));
        }
        drop(pending);
        Err(crate::Error::SaveDecisionStale(Box::new(
            self.check_save(id, path)?,
        )))
    }

    pub(super) fn publish_save_check(&self, id: DocId, mut check: SaveCheck) -> Result<SaveCheck> {
        check.token = ulid::Ulid::generate().to_string();
        *self.docs[id.0]
            .pending_save_check
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))? = Some(check.clone());
        Ok(check)
    }

    pub(super) fn consume_save_check(&self, decision: &SaveDecision) -> Result<bool> {
        // A failed attempt against another requested document still spends the
        // originating receipt. A foreign Session cannot spend our authority.
        if decision.check.scope.session != self.id {
            return Ok(false);
        }
        let Some(slot) = self.docs.get(decision.check.scope.document) else {
            return Ok(false);
        };
        let mut pending = slot
            .pending_save_check
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))?;
        // Never consume a newer check when a stale/replayed token is submitted.
        if pending
            .as_ref()
            .is_some_and(|check| check.token == decision.check.token)
        {
            pending.take();
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn save_check_scope(
        &self,
        id: DocId,
        target: &Path,
        evidence: EvidenceCheck,
    ) -> Result<SaveCheck> {
        let target = crate::save_guard::target_path(target)?;
        let target_revision = target_revision(&target)?;
        let publication = match &self.write_publication {
            crate::documents::WritePublication::Immediate => None,
            context => Some(context.logical(&target)?),
        };
        let additional_evidence = publication
            .as_ref()
            .map(|target| crate::input_evidence::check(target))
            .transpose()?
            .into_iter()
            .collect();
        let scope = SaveScope {
            evidence,
            additional_evidence,
            publication,
            finalization: false,
            session: self.id.clone(),
            document: id.0,
            document_key: self.docs[id.0].key.clone(),
            generation: self.generation(id),
            revision: self.revision(id),
            accepted_hashes: self.docs[id.0].accepted_hashes.clone(),
            target,
            target_revision,
        };
        // The digest includes all typed evidence and document/target context.
        // It is private authority, never reconstructed from a wire snapshot.
        let scope_revision = basecraft_io::hash_bytes(&serde_json::to_vec(&scope)?);
        Ok(SaveCheck {
            token: String::new(),
            scope,
            scope_revision,
        })
    }

    /// Explicit one-attempt decision, including overwrite confirmation supplied
    /// by the caller. Adapters without overwrite confirmation use `save_new_with_decision`.
    pub fn save_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(id, path, decision.known, true, false, Some(decision))
    }

    /// The same decision boundary while retaining `save_new`'s no-overwrite rule.
    pub fn save_new_with_decision(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        decision: SaveDecision,
    ) -> Result<SaveOutcome> {
        self.save_attempt(id, path, decision.known, false, false, Some(decision))
    }
}

// Unsafe/unreadable destinations remain actual target errors, never waivable issues.
fn target_revision(target: &Path) -> Result<Option<TargetRevision>> {
    Ok(match std::fs::symlink_metadata(target) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(invalid("save target must be an ordinary file"));
            }
            Some(TargetRevision {
                hash: basecraft_io::hash_file(target)?,
                stamp: crate::input_evidence::stamp(target)?,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    })
}

impl Session {
    pub(crate) fn recovery_placement(&mut self, stage: PathBuf, destination: PathBuf) {
        self.write_publication = crate::documents::WritePublication::Recovery {
            stage,
            destination,
            stores: std::sync::Mutex::new(Vec::new()),
        };
    }

    /// Observation only, not receipt authority. Called under the evidence guard
    /// immediately after a successful write or authorized finalization.
    pub(crate) fn recovery_snapshot(&self, id: DocId) -> Result<SaveCheck> {
        let target = self.resolve(&self.docs[id.0].key);
        let mut check =
            self.save_check_scope(id, &target, crate::input_evidence::check(&target)?)?;
        check.scope.finalization = true;
        check.scope_revision = basecraft_io::hash_bytes(&serde_json::to_vec(&check.scope)?);
        Ok(check)
    }

    pub(crate) fn recovery_check(&self, id: DocId, saved: bool) -> Result<SaveCheck> {
        if saved {
            self.publish_save_check(id, self.recovery_snapshot(id)?)
        } else {
            self.check_save(id, None)
        }
    }

    /// Caller holds the pinned final target reservation, then evidence guard.
    /// Reauthorizes only publication: never serializes a successful document again.
    pub(crate) fn recovery_finalize(
        &self,
        id: DocId,
        expected: &SaveCheck,
        decision: Option<SaveDecision>,
    ) -> Result<SaveCheck> {
        let valid = decision
            .as_ref()
            .map(|d| self.consume_save_check(d))
            .transpose()?
            .unwrap_or(false);
        self.check_save_source(id)?;
        let fresh = self.recovery_snapshot(id)?;
        if let Some(decision) = decision {
            if !valid || !fresh.same_scope(&decision.check) {
                return Err(crate::Error::SaveDecisionStale(Box::new(
                    self.publish_save_check(id, fresh)?,
                )));
            }
            if !fresh.complete() && !decision.acknowledge_incomplete {
                return Err(crate::Error::SaveCheckRequired(Box::new(
                    self.publish_save_check(id, fresh)?,
                )));
            }
            fresh.resolve(decision.known)?;
            self.recovery_snapshot(id)
        } else if fresh.same_scope(expected) {
            Ok(fresh)
        } else {
            Err(crate::Error::SaveCheckRequired(Box::new(
                self.publish_save_check(id, fresh)?,
            )))
        }
    }

    pub(crate) fn recovery_stores(&self) -> Result<Vec<crate::analysis_store::ArtifactRun>> {
        match &self.write_publication {
            crate::documents::WritePublication::Immediate => Err(invalid("not a recovery Session")),
            crate::documents::WritePublication::Recovery { stores, .. } => Ok(stores
                .lock()
                .map_err(|_| invalid("recovery stores unavailable"))?
                .clone()),
        }
    }
}

impl Session {
    pub(crate) fn save_recovery(&mut self, id: DocId) -> Result<SaveOutcome> {
        self.save_attempt(id, None, SavePolicy::Cancel, true, false, None)
    }
}
