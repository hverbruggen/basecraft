//! One transient save receipt owner and the common authorization implementation.
//!
//! Each live document owns exactly one non-cloning `PendingSaveAuthority`. Advisory
//! checks may be cloned/serialized, but cannot recreate its pending cell. The
//! caller resolves the origin in its existing document collection and consumes
//! before validation, locks or IO. Foreign owners and older tokens cannot spend
//! a receipt. Errors may supersede the requested document's pending check.
//!
//! Callers construct the complete fresh scope under their existing guards and
//! retain ownership of source validation, document writes, logs and publication.
//! Authorization resolves known evidence through the existing evidence owner;
//! it returns only advisory observations/warnings, never a reusable permit.
//! Finalization authorizes evidence effects without invoking a document writer.
//! The diagnostic shape (including full tagged object identities) is unchanged.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use basecraft_platform::files::ObjectIdentity;
use serde::Serialize;

use crate::input_evidence::{EvidenceCheck, InspectionIssue, SavePolicy};
use crate::{Result, error::invalid};

/// A fresh advisory check, or the fresh scope returned after refusing stale
/// consent. Fields cannot be changed to manufacture authority.
#[derive(Clone, Debug, Serialize)]
pub struct SaveCheck {
    pub(super) token: String,
    #[serde(flatten)]
    pub(super) scope: SaveScope,
    #[serde(skip)]
    pub(super) scope_revision: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SaveScope {
    pub(crate) evidence: EvidenceCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) additional_evidence: Vec<EvidenceCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) publication: Option<(PathBuf, ObjectIdentity)>,
    pub(crate) finalization: bool,
    pub(crate) session: String,
    pub(crate) document: usize,
    pub(crate) document_key: String,
    pub(crate) generation: u64,
    pub(crate) revision: u64,
    pub(crate) accepted_hashes: Vec<(String, String)>,
    pub(crate) target: PathBuf,
    pub(crate) target_revision: Option<TargetRevision>,
}

impl SaveScope {
    pub(super) fn revision(&self) -> Result<String> {
        Ok(basecraft_io::hash_bytes(&serde_json::to_vec(self)?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct TargetRevision {
    hash: String,
    stamp: basecraft_protocol::provenance::FileStamp,
}

impl SaveCheck {
    /// Builds an observation from the caller's complete guarded context, without
    /// publishing a receipt. Only the live owner can publish authority for it.
    pub(crate) fn from_scope(scope: SaveScope) -> Result<Self> {
        Self::from_scope_with_participant(scope, None)
    }

    /// Bind private participant state without changing ordinary Session diagnostics.
    pub(crate) fn from_scope_with_participant(
        scope: SaveScope,
        participant: Option<(&str, &str)>,
    ) -> Result<Self> {
        let mut scope_revision = scope.revision()?;
        if let Some((domain, digest)) = participant {
            scope_revision = basecraft_io::hash_bytes(&serde_json::to_vec(&(
                "basecraft-save-participant-v1",
                scope_revision,
                domain,
                digest,
            ))?);
        }
        Ok(Self {
            token: String::new(),
            scope,
            scope_revision,
        })
    }

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

    pub(crate) fn complete(&self) -> bool {
        self.evidence_groups().all(EvidenceCheck::is_complete)
    }

    pub(crate) fn issues(&self) -> Vec<InspectionIssue> {
        self.evidence_groups()
            .flat_map(|group| group.issues.clone())
            .collect()
    }

    pub(super) fn resolve(
        &self,
        policy: SavePolicy,
        actions: Option<&std::sync::Mutex<Vec<EvidenceAction>>>,
    ) -> Result<()> {
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
        let mut removed = std::collections::BTreeSet::new();
        for group in self.evidence_groups() {
            if group.conflict.analyses.is_empty() {
                continue;
            }
            let mut actions = actions
                .map(|actions| {
                    actions
                        .lock()
                        .map_err(|_| invalid("save action state unavailable"))
                })
                .transpose()?;
            if let Some(actions) = &mut actions {
                actions.push(EvidenceAction {
                    conflict: group.conflict.clone(),
                    policy,
                    completed: false,
                });
            }
            crate::input_evidence::resolve_group(&group.conflict, policy, &mut removed)?;
            if let Some(actions) = &mut actions
                && let Some(action) = actions.last_mut()
            {
                action.completed = true;
            }
        }
        Ok(())
    }

    /// A post-write observation may describe newly discovered issues. It must
    /// force finalization consent, never grant authority from that observation.
    pub(crate) fn require_final_decision(&mut self) {
        self.scope_revision.clear();
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
    pub(crate) recovery_observation: Option<Result<SaveCheck>>,
    pub warnings: Vec<SaveWarning>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SaveWarning {
    EvidenceNotChecked { issues: Vec<InspectionIssue> },
}

/// The single pending cell. Moving the owner retains authority; cloning or
/// deserializing advisory checks never creates another owner.
#[derive(Default)]
pub(crate) struct PendingSaveAuthority {
    pending: Mutex<Option<SaveCheck>>,
}

impl PendingSaveAuthority {
    /// Discard consent without changing document state or performing IO.
    pub(crate) fn invalidate(&self) -> Result<()> {
        self.pending
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))?
            .take();
        Ok(())
    }

    pub(crate) fn publish(&self, mut check: SaveCheck) -> Result<SaveCheck> {
        check.token = ulid::Ulid::generate().to_string();
        *self
            .pending
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))? = Some(check.clone());
        Ok(check)
    }

    pub(crate) fn decide(
        &self,
        token: &str,
        known: SavePolicy,
        acknowledge_incomplete: bool,
    ) -> Result<Option<SaveDecision>> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))?;
        Ok(pending
            .as_ref()
            .filter(|check| check.token == token)
            .map(|check| check.clone().decide(known, acknowledge_incomplete)))
    }

    /// Resolve only within the supplied live owner, before touching any cell.
    pub(crate) fn origin(owner: &str, decision: &SaveDecision) -> Option<usize> {
        (decision.check.scope.session == owner).then_some(decision.check.scope.document)
    }

    pub(crate) fn consume(&self, decision: &SaveDecision) -> Result<bool> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| invalid("save check state is unavailable"))?;
        if pending
            .as_ref()
            .is_some_and(|check| check.token == decision.check.token)
        {
            pending.take();
            return Ok(true);
        }
        Ok(false)
    }

    fn validate_decision(
        &self,
        fresh: &SaveCheck,
        decision: &SaveDecision,
        consumed: bool,
    ) -> Result<()> {
        if !consumed || !fresh.same_scope(&decision.check) {
            return Err(crate::Error::SaveDecisionStale(Box::new(
                self.publish(fresh.clone())?,
            )));
        }
        if !fresh.complete() && !decision.acknowledge_incomplete {
            return Err(crate::Error::SaveCheckRequired(Box::new(
                self.publish(fresh.clone())?,
            )));
        }
        Ok(())
    }

    /// Called under the target/evidence guards. Nonwaivable source validation
    /// runs after consent checks but before any known-evidence effect.
    pub(crate) fn authorize_write(
        &self,
        fresh: &SaveCheck,
        decision: Option<(SaveDecision, bool)>,
        policy: SavePolicy,
        validate_source: impl FnOnce() -> Result<()>,
        actions: Option<&Mutex<Vec<EvidenceAction>>>,
    ) -> Result<Vec<SaveWarning>> {
        if let Some((decision, consumed)) = decision {
            self.validate_decision(fresh, &decision, consumed)?;
            drop(decision);
        } else if !fresh.complete()
            || fresh
                .evidence_groups()
                .any(|g| !g.conflict.analyses.is_empty())
        {
            return Err(crate::Error::SaveCheckRequired(Box::new(
                self.publish(fresh.clone())?,
            )));
        }
        validate_source()?;
        fresh.resolve(policy, actions)?;
        Ok(if fresh.complete() {
            Vec::new()
        } else {
            vec![SaveWarning::EvidenceNotChecked {
                issues: fresh.issues(),
            }]
        })
    }

    /// A matching post-write observation needs no new decision. Changed scope
    /// requires a fresh receipt; explicit decisions authorize evidence only.
    pub(crate) fn authorize_finalization(
        &self,
        fresh: &SaveCheck,
        expected: Option<&SaveCheck>,
        decision: Option<(SaveDecision, bool)>,
        actions: Option<&Mutex<Vec<EvidenceAction>>>,
    ) -> Result<()> {
        if let Some((decision, consumed)) = decision {
            self.validate_decision(fresh, &decision, consumed)?;
            let policy = decision.known;
            drop(decision);
            fresh.resolve(policy, actions)
        } else if expected.is_some_and(|expected| fresh.same_scope(expected)) {
            Ok(())
        } else {
            Err(crate::Error::SaveCheckRequired(Box::new(
                self.publish(fresh.clone())?,
            )))
        }
    }
}

// Unsafe/unreadable destinations remain actual target errors, never waivable issues.
pub(crate) fn target_revision(target: &Path) -> Result<Option<TargetRevision>> {
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

/// Known-evidence writes are separate effects. An incomplete action may have
/// modified some backing ledgers before failure; it is never a rollback claim.
#[derive(Clone, Debug, Serialize)]
pub struct EvidenceAction {
    pub conflict: crate::input_evidence::EvidenceConflict,
    pub policy: SavePolicy,
    pub completed: bool,
}

#[cfg(test)]
mod participant_tests {
    use super::*;

    #[test]
    fn participant_binding_preserves_session_shape_and_rejects_changed_state() {
        let name = "session::save_authority::participant_tests::participant_binding_preserves_session_shape_and_rejects_changed_state";
        let Some(root) = crate::isolated_profile::enter(name) else {
            return;
        };
        let path = root.join("sequence.fasta");
        std::fs::write(&path, ">x\nACGT\n").unwrap();
        let mut session = crate::Session::new(&root);
        let id = session.open(&path).unwrap();
        let original = session.check_save(id, None).unwrap();
        let plain = SaveCheck::from_scope(original.scope.clone()).unwrap();
        let none = SaveCheck::from_scope_with_participant(original.scope.clone(), None).unwrap();
        assert_eq!(plain.scope_revision, original.scope.revision().unwrap());
        assert_eq!(plain.scope_revision, none.scope_revision);
        assert_eq!(
            serde_json::to_vec(&plain).unwrap(),
            serde_json::to_vec(&none).unwrap()
        );
        let a = SaveCheck::from_scope_with_participant(
            original.scope.clone(),
            Some(("sanger", "state-a")),
        )
        .unwrap();
        let b = SaveCheck::from_scope_with_participant(
            original.scope.clone(),
            Some(("sanger", "state-b")),
        )
        .unwrap();
        let other = SaveCheck::from_scope_with_participant(
            original.scope.clone(),
            Some(("other", "state-a")),
        )
        .unwrap();
        assert_ne!(a.scope_revision, plain.scope_revision);
        assert_ne!(a.scope_revision, b.scope_revision);
        assert_ne!(a.scope_revision, other.scope_revision);
        assert_eq!(
            serde_json::to_vec(&plain).unwrap(),
            serde_json::to_vec(&a).unwrap()
        );
        let owner = PendingSaveAuthority::default();
        let d = owner
            .publish(a.clone())
            .unwrap()
            .decide(SavePolicy::Cancel, true);
        let spent = owner.consume(&d).unwrap();
        assert!(matches!(
            owner.validate_decision(&b, &d, spent),
            Err(crate::Error::SaveDecisionStale(_))
        ));
        assert!(!owner.consume(&d).unwrap());
        let d = owner
            .publish(a.clone())
            .unwrap()
            .decide(SavePolicy::Cancel, true);
        assert!(
            owner
                .validate_decision(&a, &d, owner.consume(&d).unwrap())
                .is_ok()
        );
        let d = owner.publish(a).unwrap().decide(SavePolicy::Cancel, true);
        owner.invalidate().unwrap();
        assert!(!owner.consume(&d).unwrap());
    }

    #[test]
    fn poisoned_invalidation_cannot_leave_usable_consent() {
        let owner = PendingSaveAuthority::default();
        let _ = std::panic::catch_unwind(|| {
            let _guard = owner.pending.lock().unwrap();
            panic!("test-only poison");
        });
        assert!(owner.invalidate().is_err());
        assert!(owner.decide("token", SavePolicy::Cancel, true).is_err());
    }
}
