//! The check before a save and the user's answer to it.
//!
//! A save over a file that analyses used, or whose records could not all be
//! checked, needs an answer. The session shows the user a `SaveCheck` with a
//! token; the answer comes back as a `SaveDecision` naming that token. Each
//! open document holds at most one pending check, and an answer is used once:
//! a copied or older check cannot be answered. When the save runs, it checks
//! again under its locks; if anything changed, the answer no longer counts
//! and the user gets the new check. Nothing here is written to disk.
use crate::error::changed_on_disk;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use basecraft_platform::files::ObjectIdentity;
use serde::Serialize;

use crate::records::run_inputs::{InputCheck, SavePolicy, UncheckedRecord};
use crate::{Result, error::invalid};

/// What a save would affect, shown to the user before they answer. Also
/// returned when an answer was given to an older check.
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
    pub(crate) evidence: InputCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) additional_evidence: Vec<InputCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) publication: Option<(PathBuf, ObjectIdentity)>,
    /// The check made after a recovery wrote the document.
    #[serde(rename = "finalization")]
    pub(crate) after_write: bool,
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
    pub(super) hash: String,
    /// Size, modification time and identity. Change time is left out: the hash
    /// already binds the content.
    stamp: basecraft_protocol::provenance::FileStamp,
}

impl SaveCheck {
    /// A check for `scope`, not yet the document's pending check.
    pub(crate) fn from_scope(scope: SaveScope) -> Result<Self> {
        let scope_revision = scope.revision()?;
        Ok(Self {
            token: String::new(),
            scope,
            scope_revision,
        })
    }

    pub fn evidence(&self) -> &InputCheck {
        &self.scope.evidence
    }

    pub fn document(&self) -> usize {
        self.scope.document
    }

    pub fn target(&self) -> &Path {
        &self.scope.target
    }

    /// The token an answer to this check names; valid once, in this session.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The check of the target, then of each other file the save replaces.
    /// Two of them may name the same analysis; each keeps its own options.
    pub fn evidence_groups(&self) -> impl Iterator<Item = &InputCheck> {
        std::iter::once(&self.scope.evidence).chain(&self.scope.additional_evidence)
    }

    pub(crate) fn complete(&self) -> bool {
        self.evidence_groups().all(InputCheck::is_complete)
    }

    /// Whether the user answers before this save: a record could not be
    /// checked, or analyses used the file.
    pub fn needs_answer(&self) -> bool {
        !self.complete()
            || self
                .evidence_groups()
                .any(|group| !group.conflict.analyses.is_empty())
    }

    pub(crate) fn issues(&self) -> Vec<UncheckedRecord> {
        self.evidence_groups()
            .flat_map(|group| group.issues.clone())
            .collect()
    }

    pub(super) fn resolve(
        &self,
        policy: SavePolicy,
        actions: Option<&std::sync::Mutex<Vec<AppliedAnswer>>>,
    ) -> Result<()> {
        // Refuse before changing any run input if one group cannot take the answer.
        for group in self.evidence_groups() {
            if group.conflict.analyses.is_empty() {
                continue;
            }
            if policy == SavePolicy::Cancel {
                return Err(crate::Error::InputConflict(Box::new(
                    group.conflict.clone(),
                )));
            }
            if !group.conflict.options.contains(&policy) {
                return Err(crate::error::unsupported(
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
                actions.push(AppliedAnswer {
                    conflict: group.conflict.clone(),
                    policy,
                    completed: false,
                });
            }
            crate::records::run_inputs::resolve_group(&group.conflict, policy, &mut removed)?;
            if let Some(actions) = &mut actions
                && let Some(action) = actions.last_mut()
            {
                action.completed = true;
            }
        }
        Ok(())
    }

    /// A check made after writing may find something new: the user must
    /// answer it, rather than this check counting as answered.
    pub(crate) fn require_new_answer(&mut self) {
        self.scope_revision.clear();
    }

    pub(super) fn same_scope(&self, other: &Self) -> bool {
        self.scope_revision == other.scope_revision
    }

    pub(super) fn target_is_current(&self, target: &Path) -> Result<bool> {
        let same = crate::records::save_guard::target_path(target)? == self.scope.target
            && crate::records::save_guard::change_stamp(target)?.map(|stamp| stamp.0)
                == self.scope.target_revision.as_ref().map(|r| r.stamp.clone());
        #[cfg(not(unix))]
        let same = same && target_revision(target)? == self.scope.target_revision;
        Ok(same)
    }

    /// The user's answer: `known` for the analyses that used the file, and
    /// whether to save although some records could not be checked.
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
    pub(crate) known: SavePolicy,
    pub(super) acknowledge_incomplete: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct SaveOutcome {
    #[serde(skip)]
    pub(crate) recovery_check: Option<Result<SaveCheck>>,
    pub warnings: Vec<SaveWarning>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SaveWarning {
    EvidenceNotChecked { issues: Vec<UncheckedRecord> },
}

/// A document's one pending check, which an answer must match.
#[derive(Default)]
pub(crate) struct PendingSaveCheck {
    pending: Mutex<Option<SaveCheck>>,
}

impl PendingSaveCheck {
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

    /// The document an answer is for, if it belongs to session `owner`.
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

    pub(crate) fn validate_decision(
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

    /// Under the save's locks: refuse a missing, stale or insufficient answer,
    /// then run `validate_source`, then carry out the answer for the analyses
    /// that used the file.
    pub(crate) fn apply_answer(
        &self,
        fresh: &SaveCheck,
        decision: Option<(SaveDecision, bool)>,
        policy: SavePolicy,
        validate_source: impl FnOnce() -> Result<()>,
        actions: Option<&Mutex<Vec<AppliedAnswer>>>,
    ) -> Result<Vec<SaveWarning>> {
        if let Some((decision, consumed)) = decision {
            self.validate_decision(fresh, &decision, consumed)?;
            drop(decision);
        } else if fresh.needs_answer() {
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

    /// After a recovery wrote its files: a check equal to `expected` needs no
    /// new answer; otherwise an answer must be given and is carried out.
    pub(crate) fn apply_recovery_answer(
        &self,
        fresh: &SaveCheck,
        expected: Option<&SaveCheck>,
        decision: Option<(SaveDecision, bool)>,
        actions: Option<&Mutex<Vec<AppliedAnswer>>>,
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
/// The target's hash, read between two equal change stamps; none for a new file.
pub(crate) fn target_revision(target: &Path) -> Result<Option<TargetRevision>> {
    let Some(stamp) = crate::records::save_guard::change_stamp(target)? else {
        return Ok(None);
    };
    let hash = basecraft_io::hash_file(target)?;
    if crate::records::save_guard::change_stamp(target)?.as_ref() != Some(&stamp) {
        return Err(changed_on_disk(target));
    }
    Ok(Some(TargetRevision {
        hash,
        stamp: stamp.0,
    }))
}

/// The user's answer as applied to one conflict. Each run input is written
/// on its own, so one that did not complete may have changed some of them.
#[derive(Clone, Debug, Serialize)]
pub struct AppliedAnswer {
    pub conflict: crate::records::run_inputs::InputConflict,
    pub policy: SavePolicy,
    pub completed: bool,
}
